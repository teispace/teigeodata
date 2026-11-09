---
title: "Engine and catalog: layered packs, the baseline, discovery by name"
status: research
updated: 2026-11-09
owner: research-agent
revisit: 2027-02-09
---

# Engine and catalog: layered packs, the baseline, discovery by name

## 1. Question

Phase 4 builds `teigeodata-engine` and the catalog half of `teigeodata-sync`
([roadmap](../../07-planning/roadmap.md), M4). Three things are still open after
[offline loading and trust](offline-loading-and-trust.md), which settled trust, hosting, the
download architecture and the TUF shape of the catalog:

1. **Layered engine.** How the embedded baseline, bundled packs, downloaded packs and delta
   layers compose into one answer: precedence, routing a query to the right packs, merging
   results across packs, and swapping the set while queries run
   ([ADR-0011](../../03-decisions/adr/ADR-0011-layered-engine.md)).
2. **Catalog content.** Exactly what lists packs, versions, sizes, hashes, signatures and
   dependencies, inside the TUF targets chosen by
   [ADR-0037](../../03-decisions/adr/ADR-0037-keys-and-hosting.md), and how big it gets.
3. **Discovery.** An end user types "Dubai": how the library goes from that name to the pack
   address `ae-du` and a download plan, with no hosted service
   ([ADR-0010](../../03-decisions/adr/ADR-0010-addressing-and-baseline.md),
   [ADR-0012](../../03-decisions/adr/ADR-0012-offline-loading.md)).

The baseline's contents and size budget, which ADR-0010 says are "budgeted and gated" but never
numbered, belong here too.

## 2. Summary and recommendation

**Engine.** An `Engine` holds an `Arc<Snapshot>` behind `arc-swap` (1.9.4, 2026-11-07); a query
loads the current snapshot once and runs on it to the end, and an install or removal publishes
a new snapshot. A snapshot is an ordered list of **layers**, each an open pack plus its
coverage (address, bbox from the manifest), release and priority. Precedence per stable ID:
developer priority first, then the newer release, then the more specific address; a tombstone
or redirect in a winning layer hides the ID in every layer below it. The baseline is always the
lowest layer.

Routing uses facts we already have:
- `get(id)` reads the ID's 16-bit namespace (D-070) and asks only the layers of that country,
  plus the baseline.
- Point and box queries ask only the layers whose bbox holds the point or meets the box.
- `search` asks every layer whose address the caller did not filter out, takes each layer's top
  k, and merges. That is sound because the score is one additive formula for every country
  (D-052), so scores from two packs compare.

Results merge with a k-way merge that drops an ID already returned by a higher layer. The
baseline answers a point only where no installed pack covers it. Each answer carries its tier:
`Baseline` or `Pack { address, release }`.

**Baseline.** The baseline is a normal `.tgd` pack, built by the forge with the same ID registry,
so its cities have the same IDs as the full packs and merge by ID with no special case. It holds:
- countries and first-level admin units, with simplified boundaries;
- cities above a population threshold, with their multilingual names and a SEARCH section;
- a simplified world timezone REGION;
- the **locator**: a REGION whose regions are pack addresses.

A budget of **8 MB is proposed** (unverified until built; Organic Maps ships a 64 MB
`World.mwm` plus a 5.6 MB border file for the same job). The baseline is its own feature
(`baseline`), so an app can leave it out or swap in a smaller one.

**Locator.** Point → pack addresses through the REGION machinery we already have. The regions are
the coverage polygons of every address (admin units, or grid tiles such as `in-br-t07`),
simplified and **buffered** by the simplification tolerance, so near a border `regions_at`
returns both addresses (D-084 already returns every region holding a point). `reverse()` then
knows which pack is missing, and `missing_pack_for(point)` is one lookup.

**Catalog.** Keep TUF JSON ([ADR-0037](../../03-decisions/adr/ADR-0037-keys-and-hosting.md)). One
target per **downloadable asset**; the `custom` object carries a fixed, versioned schema
([§5](#5-what-we-build-ourselves-and-why)):
- address, component, release and file name;
- the raw pack's length, BLAKE3 and section-table hash;
- the transport encoding (see [download and delta](download-and-delta.md));
- `deps`, `min_reader`, bbox and licence;
- download and disk bytes per component.

The region tree comes from the addresses themselves (`ae-du` is under `ae`). Display names come
from the baseline, never from the catalog. Expected size: about 300–400 bytes per target. The
world at one asset per component and address (about 5,000 targets, an estimate) is 1.5–2 MB of
JSON, a few hundred KB gzipped (GitHub Pages serves gzip, measured below). Delegations
(hashed bins) come back only if a gate shows the catalog too large to refresh on a phone.

**Discovery.** "Dubai" → search the baseline (exact, loose and fuzzy tiers, every script) →
candidate places with IDs and points → the locator gives their addresses → the catalog gives the
targets, sizes and dependencies → `plan()`. All of it runs offline except the catalog refresh,
which is cached. Places too small for the baseline are found once an optional **world
gazetteer pack** (names of every populated place, each with its address, no geometry) is
installed. Online geocoding is not used: there is no hosted service, and a future hosted tier
can add it as one more pluggable resolver ([ADR-0025](../../03-decisions/adr/ADR-0025-hosted-ready.md)).

## 3. Findings

### 3.1 Prior art for layered read-only data

| System | Layers | Deletes | Cross-layer query | Source |
|---|---|---|---|---|
| Lucene | segments, each "a fully independent index, which could be searched separately" | a live-documents file (`.liv`) per segment; deleted documents are removed when segments merge | top-k per segment, merged by score | Lucene 10.3.0 codec docs |
| LSM trees (RocksDB and kin) | sorted runs, newest first | tombstones hide older values until compaction | k-way merge, newest version wins | standard design |
| Organic Maps | `World.mwm` (63.9 MB) and `WorldCoasts.mwm` (8.4 MB) always present, plus per-country `.mwm` | a country file is replaced whole | the world file answers where no country file is installed; search runs over the world file and every installed country (**unverified** in its source; observed app behaviour) | `data/` on GitHub, checked 2026-11-09 |
| Swiss Ephemeris (ADR-0010's model) | date-addressed files over a built-in fallback theory | n/a | the file that covers the date, else the fallback | ADR-0010 |

What carries over:
- Lucene and LSM trees both keep layers immutable, hide deleted entries with a per-layer
  structure, and pay a merge cost per query that grows with the number of layers. Both bound that
  cost with compaction. ADR-0011 already plans compaction; the gate is L5
  ([benchmarks](../../06-engineering/benchmarks.md)).
- Organic Maps is the closest product match. Its world file answers "Dubai" before the UAE map
  is installed, and its `packed_polygons.bin` (5.6 MB) maps a point to the country file to
  offer. Our baseline and locator do the same job.

### 3.2 What our packs already give the engine

- **Stable IDs with a country namespace** (D-070): `get(id)` needs no global index; the
  namespace names the country whose packs can hold the ID. IDs of places that moved between
  namespaces are found through REDIRECTS (D-075), one hop.
- **REDIRECTS** marks a retired ID as redirected (survivor) or tombstoned (0). A higher layer's
  REDIRECTS entry hides the same ID in lower layers.
- **One score for every country** (D-052, D-085): SEARCH lists rows by importance; a top-k from
  each pack merges by score without rescaling. Importance must therefore stay on one global
  scale in the forge. This is a forge invariant to state and test, not something the engine can
  repair.
- **`regions_at`** (D-084) returns every region holding a point in ID order, which is what a
  buffered locator needs.
- **The manifest bbox** (tag `0x0006`) is enough for routing point and box queries. A pack that
  crosses the antimeridian already has a defined bbox form
  ([pack format](../../04-architecture/pack-format.md)).
- **Lazy integrity:** the engine verifies sections under 1 MiB whole and larger ones per 16 KiB
  block on first read (D-078). Opening a layer costs only the head checks (L1).

### 3.3 Snapshot swap

- `arc-swap` 1.9.4 (2026-11-07, MIT OR Apache-2.0) gives lock-free `load()` of an `Arc` and
  atomic `store()`. A reader never blocks a writer and the reverse. A loaded guard keeps the old
  snapshot alive until the query ends. Old packs are unmapped only when the last snapshot that
  holds them drops. That is what the install rule needs: "a file that may be mapped is never
  written over" ([pack format](../../04-architecture/pack-format.md)).
- The alternative is `RwLock<Arc<Snapshot>>` from `std`. It needs no dependency, but a writer
  waits for readers, and a lock is taken on every query (tens of nanoseconds, against queries of
  40–150 ns, D-083, D-100). The A/B is cheap and belongs in L5.
- `wasm32-unknown-unknown` without threads has no contention, so either works there; `arc-swap`
  builds on it (**unverified**; Z2 measures).

### 3.4 Precedence

ADR-0011 says "a newer release wins by default; the developer can set priorities". Made exact:

1. A developer priority (an integer per layer, default 0) wins first.
2. Then the layer with the newer `release`. Release strings are ordered as dotted numbers
   (`2026.11.0 < 2026.12.0`), never as text.
3. Then the more specific address (`ae-du` over `ae`): a city pack beats the country pack it
   overlaps.
4. Then the delta layer over the base layer it patches.
5. Ties are broken by the address text, so the order is total and deterministic.

The baseline is always last. The winning layer for an ID decides everything about it:
- a tombstone there hides the ID everywhere;
- a redirect there sends `get` to the survivor;
- otherwise its row is the answer.

**Coverage beats existence for points.** For `reverse()` and `admin_at()`, the question is not
"which layer has a result" but "which layer covers this point". A Dubai city pack and an
older UAE country pack both cover a point in Dubai; the city pack wins by rule 3. The baseline
covers everything, so it answers only where no installed pack covers the point. That rule makes
the baseline invisible once the right pack is present, which is ADR-0010's "installed packs
override the baseline automatically".

### 3.5 Cross-pack queries

| Query | Layers asked | Merge |
|---|---|---|
| `get`, `get_many` | the namespace's packs, then the baseline | first winning layer per ID |
| `reverse` (nearest place) | packs whose bbox holds the point, widened by the search radius | k-way merge by distance; drop IDs already taken by a higher layer |
| `admin_at`, `tz_at` | the covering pack with the highest precedence; the baseline if none | one answer, with its tier |
| `within_bbox`, `within_radius`, `nearby` | packs whose bbox meets the box | union, dedupe by ID, limit |
| `search`, `autocomplete` | all packs (filtered by address if asked) | per-pack top k, merge by score, dedupe by ID |
| `children`, `ancestors` | the packs holding each ID (namespace) | follow parent IDs across packs |

- **Cross-pack hierarchy:** a city pack's places have parents in the country pack, so
  `ancestors` crosses packs by ID. Packs declare these dependencies (ADR-0011), and the planner
  installs them together. Two ways to carry the link were compared on 2026-11-09 (D-143):

  | Option | Format | `ancestors` with only the city pack | `children` of a unit split over packs | Cost |
  |---|---|---|---|---|
  | **A. Ancestor stubs** | none: each pack holds its places' ancestors as rows (the split gate already did this for admin units) | the whole chain, offline | union of every layer holding the unit's ID | a few rows per pack (a chain is 3–6 deep) |
  | B. Parent IDs | a new HIERARCHY column, the parent's ID where it lives in another pack | stops at the pack edge until the country pack is installed | a scan of every layer's parent-ID column, or a reverse index per pack | 8 B per root row, plus the reverse index |

  **Chosen: A.** A city pack downloaded alone still answers its full chain (an end user picks
  "Dubai", ADR-0012), the format does not change, and `children` needs no reverse index. The
  engine also follows IDs across packs, so a pack whose stub chain is cut short defers to a
  layer that knows the parent; a newer pack that moves a child removes it from its old parent.
  Measured on the Nepal main pack split by ID into 1, 3 and 10 layers: `get` + `ancestors`
  307, 967 and 2,396 ns; `get` + `parent` + `children` (198 children on average) 6.9, 34.6 and
  104.6 µs ([phase-4 benchmarks](../benches/phase-4.md#cross-pack-hierarchy-d-143)).
- **Duplicates:** the baseline's cities and a full pack's cities share IDs, so dedupe by ID
  removes them. Two country packs never share a namespace's IDs unless a place moved, which
  REDIRECTS handles.
- **Allocation:** per-layer top-k buffers and the merge heap live in a caller-owned scratch, as
  `DemScratch` does today (D-104), so the zero-allocation gate (M1) can hold.

### 3.6 The baseline and the locator

What ADR-0010 lists, with sizes we can already estimate from Nepal and the world benches:

| Part | Basis for the estimate | Estimate |
|---|---|---|
| Countries and first-level units, simplified boundaries | ARCS 3.4–4.5 B per vertex (D-097, D-103); world admin1 at a 500 m–1 km tolerance, vertex count **unmeasured** | 1–3 MB |
| Cities and their names and search | about 55 B per place for PLACES, NAMES, SEARCH and indexes on Nepal (summary of the 2026-11-09 build); tens of thousands of cities with more names each | 2–4 MB |
| World timezones, simplified | the full world REGION is 25.7 MB (B9); a simplified one at the baseline's precision is a fraction | 1–2 MB |
| Locator | a REGION over buffered coverage polygons, roughly the admin1 vertex count | 0.5–1.5 MB |

Total: **5–10 MB, all estimates.** The [baseline note](baseline.md) (2026-11-09) pins the
sources and measures the boundaries: world admin 1 at 1 km is 372,470 vertices counted per ring,
at most 1.3–1.7 MB. The proposed budget is 8 MB, measured by Z1 on the built
baseline. It is a separate asset from the library's code, for two reasons:
- **Native:** `include_bytes!` into a feature-gated module, read through the same unaligned
  reader as any pack.
- **WASM:** the baseline is fetched or bundled as a file, so it does not inflate the module that
  every page loads (Z3).

The baseline is a pack, so it is signed and verified like any other, and an app can replace it
with a bigger or smaller one built by the forge. The release that the library embeds is
recorded in its version.

**Locator design.**
- Each address's coverage polygon (its admin unit, or its grid tile) is simplified with
  tolerance `t` and buffered by `t`.
- A point within `t` of a border is inside two regions, so `regions_at` returns both addresses,
  and the engine asks both packs.
- The cost of `t` is a few extra candidate packs near borders, never a wrong one.
- Grid-tile addresses (ADR-0010's `in-br-t07`) are rectangles, so they are exact and cheap.

### 3.7 Discovery: "Dubai" → `ae-du`

1. `catalog.find("Dubai")` runs the baseline's SEARCH: exact, loose, then fuzzy (D-086). Every
   script is covered (D-077), so "دبي" works too.
2. Results are places with IDs, points and kinds: Dubai the city, Dubai the emirate (`AE-DU` in
   ISO 3166-2), and perhaps other places with the name.
3. The locator maps each point (or each admin unit's ID) to addresses. Both Dubais give `ae-du`.
4. The catalog gives the targets of `ae-du`: components, release, download and disk bytes, and
   dependencies (for example `ae.admin`). The UI can show the place, its country, the download
   bytes and the disk bytes from data the library already has.
5. `plan()` adds dependencies, subtracts what is installed or reusable, and checks storage.

What the baseline cannot find: a village that is not in it. The options:
- the optional **world gazetteer pack**: names and address of every populated place, without
  geometry. Its size from the Nepal figures, roughly 30–40 B per place compressed, is **to be
  measured**.
- **coordinates:** an app can pass a point (from the device location or a map tap), and the
  locator works on any point;
- **a hosted resolver:** later, through the same `Resolver` trait, and never required.

### 3.8 Catalog content and size

Fields per target, in `custom` (TUF puts `length` and `hashes` outside it):

| Field | Why | Bytes (JSON, estimate) |
|---|---|---|
| `address`, `component`, `release`, `file` | identity and physical name (ADR-0010) | 80 |
| `pack_len`, `pack_blake3` | the installed file, which may differ from the transport asset | 90 |
| `sections` | BLAKE3 of the section table: lets a client skip an update whose sections it already has | 70 |
| `transport` | `"zst-seekable"` or `"raw"`, plus the seek table's length ([download and delta](download-and-delta.md)) | 40 |
| `deps` | addresses and components this one needs | 30 |
| `min_reader` | format major it needs | 15 |
| `bbox` | routing before install; coverage display | 50 |
| `licence` | main (`odbl`) or lite (`cc-by-4.0`) | 20 |

That is about 400 bytes per target with the TUF wrapping. Two catalogs, main and lite, are
separate targets files under one root, because an app chooses its licence once.

**Measured on 2026-11-09:** GitHub Pages answers `Accept-Encoding: zstd, br, gzip` with
`content-encoding: gzip` and `vary: Accept-Encoding`. JSON of this kind usually shrinks 5–10
times under gzip (**unverified** on our catalog; S1 measures it).

### 3.9 Releases and the catalog

- Each target's `file` carries release and hash (ADR-0010), so old and new releases sit side by
  side on the host. A client with release N can still find N's files while N+1 is published.
- `check_updates()` compares installed `(address, component, release)` with the catalog.
  An update whose `sections` hash equals the installed one costs nothing. One whose head
  differs runs the section plan of [download and delta](download-and-delta.md).

## 4. Options compared

**Snapshot swap.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `arc-swap` | lock-free reads; writers never wait for readers | one dependency | MIT OR Apache-2.0 | 1.9.4 (2026-11-07) | **Use**, if L5 shows a gain |
| `std` `RwLock<Arc<_>>` | no dependency | a lock per query; writer waits | std | stable | Fallback, A/B in L5 |
| Merge at load into one structure | fastest per query | slow start, memory spikes (ADR-0011 rejected it) | n/a | n/a | Reject |

**Discovery by name.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Baseline SEARCH + locator | offline; every script; same IDs as packs; no new code beyond the locator | only places in the baseline | ours | SEARCH as built (D-085) | **Use** |
| Names in the catalog (Organic Maps' `affiliations`) | no baseline needed | catalog grows by names in many scripts; a second search code path | n/a | Organic Maps `countries.json`, 284 KB | Reject |
| Optional world gazetteer pack | finds small places offline | a download of its own (size to measure) | ours | n/a | **Use**, optional |
| Online geocoder | finds anything | needs a hosted service (none now); privacy | n/a | n/a | Later, behind a trait |

**Locator.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| REGION over buffered coverage polygons | exact code we have; `regions_at` gives every candidate | buffer width to tune | ours | D-083, D-084 | **Use** |
| Fixed grid of cells → address lists | trivial | coarse at borders; size grows with resolution | ours | n/a | Reject |
| Bboxes from the catalog | no extra data | wrong near every border | n/a | n/a | Routing hint only |

**Catalog encoding.** Settled by [offline loading and trust](offline-loading-and-trust.md): TUF
JSON on the wire, a parsed binary cache on the device.

## 5. What we build ourselves, and why

1. **`teigeodata-engine`:** the layer list, the precedence rules of §3.4, the routing table of
   §3.5, the k-way merges and the snapshot swap. No crate does federated queries over our
   format.
   Proof:
   - metamorphic tests: splitting a pack into two layers, or reordering layers of equal
     priority, never changes an answer (ADR-0011);
   - a differential test against one compacted pack;
   - L5 with 1, 3 and 10 layers.
2. **The baseline build** in the forge: the same stages, with a population threshold and
   simplification tolerances as data, nothing per country in code (P16).
   Proof: Z1 within the budget; every baseline city has the same ID as in its full pack.
3. **The locator:** a REGION built from buffered coverage polygons.
   Proof: no point of a held-out set (country packs' places and border-adjacent probes)
   misses its pack; the extra-candidate rate near borders is reported.
4. **The catalog `custom` schema v1** (§3.8), with a JSON schema file checked in, generated by
   the release tooling and validated by the client before use.
5. **`Resolver` trait** for discovery: the baseline resolver by default, the gazetteer when
   installed, a hosted one later.

## 6. Impact on the plan

- **Phase 4** ([phase file](../../07-planning/phases/phase-4.md)): steps for the engine, the
  baseline, the locator and the catalog schema.
- **ADR-0010:** a refinement (the baseline is a signed pack with registry IDs; the locator is a
  buffered REGION; budget 8 MB proposed). The decision-log row comes when the baseline is
  measured.
- **ADR-0011:** precedence made exact (§3.4); coverage beats existence for point queries.
- **Forge invariant:** importance on one global scale (feeds cross-pack search merge); add a
  check when a second country is built.
- **Architecture:** a new `docs/04-architecture/engine.md` when the engine lands (listed in the
  overview's "pages to come").
- **Benchmarks:** L5 gains the `arc-swap` against `RwLock` A/B and the 1/3/10-layer rows.

## 7. Risks and open items

- **Baseline size** is the biggest unknown; every figure in §3.6 is an estimate. If it exceeds
  8 MB, tiers are possible: `baseline-min` with countries, timezones and the locator; and
  `baseline` with cities as well.
- **Global importance scale:** if the forge normalises importance per country, cross-pack search
  ranks wrongly without any error. Needs a test with two countries.
- **Merge cost** grows with the number of layers; delta layers make that worse (see
  [download and delta](download-and-delta.md)). The compaction policy depends on L5.
- **Locator buffer** too small misses a pack at borders; too large asks extra packs. Tune on the
  border probes.
- **Disputed areas:** the locator follows the Nepal perspective
  ([ADR-0006](../../03-decisions/adr/ADR-0006-perspective.md)), like every admin layer; a claimed
  area may map to two addresses, which `regions_at` already allows.
- `arc-swap` on WASM without threads: the engine with `shared` builds for
  `wasm32-unknown-unknown` (2026-11-09); running it there is checked with the bindings.
- The catalog size and its gzip ratio are estimates until S1 runs on a generated world catalog.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| arc-swap | 1.9.4 (2026-11-07) | 2026-11-09 | crates.io API |
| memmap2 | 0.9.12 (2026-10-21) | 2026-11-09 | crates.io API |
| serde_json | 1.0.154 (2026-10-11) | 2026-11-09 | crates.io API |
| Lucene | 10.3.0 codec docs | 2026-11-09 | lucene.apache.org |
| Organic Maps data files | `master`: `World.mwm` 63,929,894 B, `WorldCoasts.mwm` 8,351,567 B, `packed_polygons.bin` 5,643,518 B, `countries.json` 284,512 B (`"v": 260714`) | 2026-11-09 | GitHub contents API |
| GitHub Pages content encoding | gzip for `Accept-Encoding: zstd, br, gzip` | 2026-11-09 | curl against `rust-lang.github.io` |

## 9. Sources

- [Lucene 10.3.0 codec package summary](https://lucene.apache.org/core/10_3_0/core/org/apache/lucene/codecs/lucene103/package-summary.html), Apache Lucene, accessed 2026-11-09.
- [Organic Maps `data/`](https://github.com/organicmaps/organicmaps/tree/master/data), Organic Maps, accessed 2026-11-09 through the GitHub contents API.
- [arc-swap](https://crates.io/crates/arc-swap), crates.io, accessed 2026-11-09.
- Internal: [offline loading and trust](offline-loading-and-trust.md), [block hashes](block-hashes.md), [pack format](../../04-architecture/pack-format.md), [B9 region](../benches/phase-2.md), decisions D-052, D-070, D-075, D-083 to D-086, D-100, D-104 in the [decision log](../../03-decisions/log.md).
- Live probe with curl of `https://rust-lang.github.io/rustup/`, 2026-11-09.
