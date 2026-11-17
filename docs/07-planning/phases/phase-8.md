---
title: "Phase 8: hardening and release"
status: accepted
updated: 2026-11-15
---

# Phase 8: hardening and release

**Goal:** a 1.0 that strangers can put in front of untrusted input, depend on through semver,
and learn from its docs alone, released with Nepal's data. Exit: milestone **M8 = v1.0.0**
([roadmap](../roadmap.md)).

**Research:** [hardening and the v1.0.0 release](../../02-research/notes/hardening-and-release.md)
(2026-11-15); D-331.

**Order:**

1. The fuzzing campaign, which may change code everything else measures.
2. The security review, then the performance pass on the hardened code.
3. Docs and the API review, which freeze the surface.
4. The first data release and v1.0.0.

Phase 3's 3.7 and 3.8 are on this path (the data release needs them) and run between the others
while they wait on labels (Q-017). Long runs go to GitHub's runners, never to this shared
machine (D-080, [AGENTS.md](../../../AGENTS.md) §7). Nothing names a country in code (P16).

## Steps

### 8.0 Research

- [x] The hardening note: fuzzing gaps, supply chain, performance gates that never ran, docs,
  the release path, versions live (2026-11-15)

### 8.1 Fuzzing campaign

- [x] Every failed nightly accounted for:
  - `transport_table` (2026-11-09, 2026-11-10), fixed in 6a3ff32;
  - `mutate` (2026-11-14), fixed in 685495a.

  Both were harness faults. Their inputs replay in each crate's `found_by_libfuzzer` test; the
  `mutate` one fails with its fix reverted (2026-11-15)
- [x] `engine` (2026-11-15). One or two sections of a seed pack are edited and the pack is
  rewritten by the writer so its hashes hold (`teigeodata_format::fuzz::edit_section`, one
  mutator beside `mutate`). The pack is opened with `Layer::from_bytes` and asked every query.
  It runs under `cargo test` (seeded, five seeds) and libFuzzer (nightly matrix, 13 targets).
  - **Seeds:** the four binding fixtures and `fuzz/seeds/extra.tgd` (SUBSTRING, DEM_COARSE),
    blessed by `tests/fuzz.rs`. A test fails if the seeds lack a kind the engine reads.
  - **Two reader faults found in the first minutes**, each fixed and kept in
    `found_by_libfuzzer` (red with the fix reverted):
    - an item count near 2^64 overflowed `count + 1` in DEM, ARCS and POLYGONS
      (`columns::boundaries`);
    - a DEM grid's tile side was checked only by `check`, which queries never run, so a query
      shifted by 2^200. `Dem::open` now checks every grid, model and threshold, in O(grids).
  - **Then a clean run:** 300 s, 331,057 inputs, coverage 8,506, peak RSS 403 MB (this machine,
    2026-11-15). The real faults stand in for the planted one: the target reaches the decoders'
    arithmetic
- [x] `payloads`, in the `engine` target rather than a second harness (2026-11-15). Each edited
  section also gets its full check (`check::sections_where`, which selects sections, so
  REGION's 90 ms debug check does not run for an edit elsewhere). That reaches every kind's
  `open` and `check`.
  - **The seeds hold all 22 kinds except BLOBS**, which the format's `sections` harness covers.
    The extra seed gained ARCS, ARCS_OVERVIEW and COMPONENTS.
  - **Every seed is held whole:** `check::sections` passes and the engine opens it. That found
    the outline fixture's hole stored counter-clockwise, against `Polygons::check`. The
    fixture is fixed, and no parity answer changed, since queries re-orient.
  - **Run:** 300 s clean, 149,633 inputs, coverage 10,668 (8,506 without the checks), peak
    RSS 405 MB
- [x] `trust` (2026-11-15). A valid repository is built with fixed keys (`repo::*`): root,
  targets with one catalog entry, snapshot and timestamp. One role is then attacked in one of
  three ways: its file replaced by fuzz bytes, one value of its `signed` part changed and
  signed again with its own key, or signed by a key no role lists. The roles after it are
  rebuilt on the attacked file, so their hashes hold and the checks behind them are reached.
  The chain loads as a client loads it, then the catalog.
  - **Properties:** no panic; an untouched repository loads whole (red on a sample entry
    whose seek table lay past the asset); an accepted entry reads back from its own `custom`
    unchanged.
  - **Run:** 300 s clean, 88,146 inputs (292 a second, each signed again with Ed25519),
    coverage 3,630, peak RSS 437 MB
- [x] `ffi` (2026-11-15). `teigeodata_ffi::fuzz::abi` opens the four binding fixtures through
  `tg_engine_open_bytes` and calls every one of the 33 `tg_engine_*` functions as C does. The
  arguments come from the fuzz bytes:
  - strings that are not UTF-8, hold NUL bytes, or are null with a length;
  - real IDs, zones and codes, so calls reach the queries;
  - coordinates that are NaN, infinite or past the poles, and limits up to `u32::MAX`;
  - option records with any field set, missing layers and a null handle;
  - file paths that cannot exist (hex-named, so nothing outside is read).

  A stopped panic (`TG_ERROR_INTERNAL` with its own message) fails the harness. Every buffer is
  freed. `the_harness_calls_every_engine_function` reads the generated ABI and fails on a
  function the harness does not call (planted: `by_iso` removed). Store and job functions are
  left out: they reach the file system and the network.
  - **Run:** 300 s clean, 507,361 inputs, coverage 7,758, peak RSS 386 MB. `u32::MAX` limits
    allocate nothing unbounded
- [x] `mcp` (2026-11-15). The MCP server runs over the synthetic fixture. Each input either
  calls one tool (`Mcp::call`, `tools/call` without a transport) or reads a resource URI made
  from the fuzz bytes.
  - **Arguments** are built from the tool's own input schema, so a new tool is fuzzed with no
    change. Each property gets any JSON type: numbers past every bound, real IDs and zones,
    arrays, objects, null and unknown keys.
  - **Property:** no answer is `internal`. Planted: an internal error in `timezone` was found
    at once.
  - **Run:** 300 s clean, 167,048 inputs, coverage 5,215, peak RSS 453 MB.
  - **Not fuzzed here:** the JSON-RPC framing is rmcp's own (`=3.10.0`), and the
    `mcp-conformance` workflow tests it against the specification's suite
- [x] `fuzz-campaign.yml`: weekly and by dispatch, 3 h per target, a job each, with coverage
  reports as artifacts. Gate: every section kind's decoder reached, the coverage recorded here
  - **Built (2026-11-15):** the workflow ([ci.md](../../06-engineering/ci.md#fuzzing-campaign))
    and its gate `cargo xtask fuzz-coverage`. The gate fails on a decoder no target reached and
    on a kind with no decoder entry (both planted). It read a real `llvm-cov` export locally
    (the `sections` target on whole-pack seeds, rightly red: that harness takes payloads)
  - **First run (2026-11-15, run 43685581742, a 300 s dispatch to prove the pipeline):** all
    16 targets clean and the gate green. Every one of the 24 section kinds' decoders is
    reached. The best target per decoder covers these shares of its lines (kinds that share
    a file share a row):

    | Decoder | Lines | Best target |
    |---|---:|---|
    | `region.rs` (REGION, ADMIN_REGION) | 74.6 % | engine |
    | `blobs.rs` | 74.4 % | sections |
    | `polygons.rs` | 73.6 % | engine |
    | `place_index.rs` | 73.3 % | engine |
    | `hierarchy.rs` | 67.8 % | sections |
    | `postal.rs` | 63.8 % | engine |
    | `arcs.rs` (ARCS, ARCS_OVERVIEW) | 63.4 % | engine |
    | `places.rs` | 62.5 % | sections |
    | `idmap.rs` | 59.8 % | sections |
    | `iso_codes.rs` | 54.8 % | engine |
    | `blocks.rs` | 53.7 % | transport_asset |
    | `place_tz.rs` | 50.5 % | engine |
    | `components.rs` | 48.1 % | engine |
    | `tz_rules.rs` | 47.8 % | engine |
    | `names.rs` | 45.4 % | engine |
    | `redirects.rs` | 45.3 % | sections |
    | `search.rs` (SEARCH, SUBSTRING) | 39.0 % | engine |
    | `elevation.rs` | 39.0 % | engine |
    | `dem.rs` (DEM, DEM_COARSE) | 37.6 % | engine |
    | `poly_index.rs` | 35.5 % | engine |

    - Inputs per target ranged from 17,276 (`transport_asset`) to 102.7 M (`manifest`).
    - The weekly 3 h runs replace these numbers. Lines not reached are mostly the deep
      checks' refusal branches and the writer-only code in the same files.
    - The lowest three (`poly_index.rs`, `dem.rs`, `search.rs`) are the next seeds to add:
      [ideas](../../01-product/ideas.md)

### 8.2 Security review

- [x] [The threat model](../../06-engineering/threat-model.md) (2026-11-15): seven boundaries
  (pack bytes, the update chain, the C ABI, the bindings, the servers, the store, the
  pipeline), each with what an attacker controls, what must hold and the gates that show it.
  Writing it found `store::pack_name`'s path check untested: now
  `an_asset_name_never_leaves_the_store` (13 names, red without the `..` check). Its open rows
  are the server limits below and zizmor, Scorecard and semver checks
- [x] `cargo xtask unsafe-audit` (2026-11-15, D-333): [the inventory](../../06-engineering/unsafe-inventory.md)
  of 264 blocks and impls in 8 files, each with its `SAFETY:` argument and the tests that reach
  it, on every `cfg`. It fails on a block without an argument and on a file not reviewed (both
  planted); fast-check runs `--check`. The clippy lints `undocumented_unsafe_blocks` and
  `unnecessary_safety_comment` are on in every lint table
- [x] Resource limits on every server route and MCP tool (2026-11-15, D-334,
  [research](../../02-research/notes/server-limits.md)). The threat model found that
  `axum::serve` sets no hyper timer, so slow heads and idle connections were unbounded, and
  that nothing capped requests in flight. `http::serve` is now its own accept loop with
  hyper's bounds:
  - a 10 s header timeout, which also closes idle connections;
  - HTTP/2 pings and 32 streams;
  - 1,024 connections.

  Requests past 256 in flight are shed at once (503, `Retry-After`). `tests/limits.rs` has a
  test per bound, each red with its bound removed. The load gate passes its budgets with the
  bounds in place (world baseline, search at c=1: 17,137 req/s, 17,299 before). The first
  version of the in-flight cap was per route (axum layers each route on its own), and the test
  caught it
- [x] `zizmor` on every workflow in fast-check, findings fixed or each one justified
  (2026-11-15). The first run found one high finding: the WebAssembly tools action wrote
  `GITHUB_PATH`, and `wasm-opt` is now linked into cargo's bin directory instead. It also gave
  eight `self-repository` warnings (`./` reusable calls; the new `$/` form is not relied on
  yet) and one `adhoc-packages` (the exact npm upgrade trusted publishing needs), each now
  carrying an inline ignore with its reason
- [x] OpenSSF Scorecard weekly on `main`; the checks only the maintainer can satisfy listed
  (2026-11-15). First run 5.3 of 10 (run 43689300462). Fixed after it:
  - Vulnerabilities: 25 npm advisories in the WebAssembly test runner (`undici` 7.28.0 and
    `sharp` 0.35.2, pinned by miniflare), moved to 7.30.2 and 0.35.5 by `overrides`. One
    unreached `memmap2` advisory in the forge has an `osv-scanner.toml` entry.
  - SAST: the `codeql` workflow.
  - Security-Policy: a reporting link.
  - Pinned-Dependencies: `curl | sh` rustup in four Alpine jobs, now Alpine's signed `rustup`
    package.

  Left to the maintainer: Branch-Protection, Code-Review, CII-Best-Practices, and
  Signed-Releases (the first release). Maintained (the repository's age) and CI-Tests (no
  pull requests yet) come with time
  for them
- [x] The review's findings fixed, each with its test; `SECURITY.md` states the 1.0 support
  policy (2026-11-15: no row of the [threat model](../../06-engineering/threat-model.md) is
  open; each boundary names its tests and gates)

### 8.3 Performance pass

- [x] A profile of each Q and L suite on Nepal's pack and the world baseline; the three largest
  self-time items per suite recorded in [benches/phase-8](../../02-research/benches/phase-8.md).
  **Changed (D-338):** Instruments' Time Profiler through `cargo xtask profile` in place of
  `samply`, which is not installed; Nepal's lite pack stands in for its main pack, which is
  rebuilt in CI only
- [x] Each change justified by its profile and landed with its wall-clock before and after
  (alternating A/B) and its instruction count from the `counts` workflow: `Sorted::find` in
  `u64` (get −6% Nepal, −4% world), `Packed::parse`'s directory check vectorised (open −14%,
  −17%); a direct-read block search rejected (+5% on Nepal)
- [x] The scaling gate (`cargo xtask scaling`): throughput at 1, 2, 4 and 8 threads on one
  `Shared` engine, recorded for the measured page (`site/data/measured/scaling.json`). Fails
  under 1.5× at four threads; a planted global lock gave 0.76×
- [x] Suite C, the competitive report (`competitive.yml`, by dispatch on GitHub runners):
  Nominatim and Photon on Nepal's extract, `tzf-rs` and a GeoNames-backed library, on the
  golden queries. Medians of repeated runs, with each rival's version and configuration.
  **Done (2026-11-15, D-340):** `tools/competitive` and the workflow; the first run on GitHub
  (43697996108) is in [suite C §3b](../../02-research/notes/competitive-suite.md), with the
  bias of its golden set stated

### 8.4 Docs complete and the API frozen

- [x] The 1.0 API review: every IDL item read once for name, shape and error; changes made
  before the freeze, each through the bindings' `--check`. **Done (2026-11-15, D-341):**
  `within_radius`'s `meters` is `radius_m` and `postal_prefix`'s `input` is `prefix`; the
  rest kept, with the reasons in the decision
  - Found while writing `semver.md`: `teigeodata_api::ErrorCode` is an exhaustive enum, and the
    generated JVM glue matches it exhaustively, so a new error code would break callers. Make
    every public enum that may grow `#[non_exhaustive]`, with a fallback arm in each
    generator. **Done (2026-11-15, D-337):** 27 enums in 9 crates marked. The closed ones
    stay exhaustive: the grid markers, FOR/delta/ramp, redirect fates, tz rule forms and
    local times, FST walks and TUF roles. Two mappings that were copied in two crates are now
    one: `CodeKind::name` and `Health::name`. Each binding maps an unknown code to
    `Internal`
- [x] The format freeze (Q-028): each of the format benchmarks B1–B14 recorded or retired with
  its reason (D-029), then `FORMAT_MAJOR = 1`, `READER_MAJOR = 1` and `pack-format.md` out of
  draft, with the fixtures and seeds written again. **Done (2026-11-15, D-335, D-336,
  [research](../../02-research/notes/format-freeze.md)):**
  - Twelve benchmarks recorded; B11 outside Nepal and B14 retired, since they measure the
    forge, not the bytes.
  - The audit found that every section refused an unknown column, which would have broken
    older 1.x readers on any added column. Column IDs from `0x8000` up are now extension
    columns that readers skip.
  - It also found `tgd verify --deep` skipping `TZ_RULES`.
  - Format-0 packs stay readable (`tests/compat.rs`), and a client refuses a pack for a newer
    reader before downloading it.
- [x] `docs/06-engineering/semver.md`: what 1.0 promises (Rust API, C ABI, pack format, catalog,
  HTTP and MCP), MSRV and deprecation
  - Done (2026-11-15): [semver.md](../../06-engineering/semver.md), with each surface's
    compatible changes, its breaking changes and its gate; what is not covered; deprecation;
    MSRV
- [x] `cargo-semver-checks` in `release.yml` from the first tag, beside the ABI gate (D-285).
  Built (2026-11-15): `cargo-semver-checks` 0.51.0 on every published crate against the last
  `v*` tag, a warning in a dry run. First clean run in the 1.0.0 dry run 43719097976, after
  three fixes its earlier runs found: a full clone (a partial one hides blobs from its git
  library), a stand-in pack for the engine's `baseline` feature, and only the crates the last
  tag holds (`teigeodata-server` is new since `v0.3.1`)
- [x] A docs audit: every guide, concept and reference page read against the code once; the
  1.0 section of `CHANGELOG.md` (2026-11-15, D-343)

### 8.5 The first data release

- [x] Phase 3.7: the lite build from clean-room sources only, and Nepal's `ACCURACY.md` against
  held-out truth (Q-017): both builds' pages on the CI-built packs (D-339, D-348)
- [x] Phase 3.8: M3 met (D-350)
- [x] Each place's population in the packs and every API (D-351): COMPONENTS component 1, the
  forge writes and reads it back (G-pop), the engine, the API columns `population` and
  `population_kind`, `tgd` and the servers; found missing while staging the data release
- [x] Nepal's main and lite packs as a data release with their accuracy summary, rebuilt in CI
  with populations: [data-2026.11.15](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.15)
  (a GitHub pre-release from `data-build` run 43733977409 at `6b234d4`; every asset's digest
  checked against the run's `SHA256SUMS`; lite has the places of data-2026.11.14 by `tgd diff`)
- [ ] The same packs through the signed catalog, after the key ceremony (phase 4.3)
- [x] A dry run of `release.yml` end to end at the release commit: run 43719097976 on
  `chore/release-1.0.0` (the version bumped to 1.0.0 with `cargo xtask bump`, D-346), 53 jobs
  green, the ABI and Rust API gates clean. Rerun on the rebased branch at tag `v1.0.0-rc.2`
  (main 404a0cc, the population parity fix and the data release): run 43741537528, 52 jobs
  green, 2026-11-16
- [x] The release environment and registry accounts (5.10c), set up with the maintainer
  2026-11-16; the runbook records them
- [ ] The maintainer: the key ceremony (phase 4.3), Pages (`DOCS_DEPLOY`)

### 8.5b Packs at run time on every platform (TG-24, TG-25, TG-26; D-362)

- [x] Research: [the store in the browser](../../02-research/notes/browser-store.md): an async
  driver in a Worker over the sans-I/O cores, OPFS, Web Locks, the baseline first
- [ ] Refresh and store policy as sans-I/O plans in `teigeodata-sync`, the native client moved
  onto them with its tests unchanged
- [ ] `teigeodata-wasm` `store` feature and `store-worker`: install, update, remove, pin, repair,
  status over `fetch` and OPFS; browser tests (Chromium, Firefox, WebKit) against a local CORS
  mirror and a throwaway-key catalog
- [x] TG-26 as a forge profile, not `extract` (search keys must be rebuilt with each country's
  keying rules, which live in the forge): `data/embed/app.yaml`, a part may leave its outlines
  out; measured 2026-11-17 in [sizes](../../05-data/sizes.md#embedded-layers-built-from-profiles)
- [ ] A public page on running a mirror (TG-24); the catalog live after the key ceremony

### 8.6 Exit review

- [x] v1.0.0 tagged and published on GitHub (D-352: registries off) and checked from a scratch
  project with the `git` + `tag` dependency: [release run 43841066655](https://github.com/teispace/teigeodata/actions/runs/43841066655)
  (52 jobs green; registries skipped), tag at the release commit `5f7d916`, 79 files; the C
  archive's `SHA256SUMS` line and attestation verified; a new project on `tag = "v1.0.0"`
  opens `np-places-lite.tgd` of data-2026.11.15 and reads Nepal 29,164,578 (stated), 2026-11-16
- [x] The registries: 1.0.1 (D-358) published on every registry, the first publish per
  registry by hand (pub.dev from `main`, D-361), installs checked; **1.1.0 published everywhere
  from CI by trusted publishing** ([run 43931508857](https://github.com/teispace/teigeodata/actions/runs/43931508857),
  pub.dev run 43943423993), 2026-11-16. A new patch release replaced the planned "publish an
  existing tag" mode: simpler, and it proved trusted publishing end to end
- [ ] M8 criteria met, each with its evidence; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Phase 8 drafted from the [hardening research](../../02-research/notes/hardening-and-release.md) | D-331 |
| 2026-11-15 | 8.1 engine target done; two DEM, ARCS and POLYGONS faults fixed | D-332 |
| 2026-11-15 | 8.1 payloads folded into the engine target; seeds hold every kind but BLOBS | D-332 |
| 2026-11-15 | 8.1 trust target done | D-331 |
| 2026-11-15 | 8.1 ffi target done | D-331 |
| 2026-11-15 | 8.1 mcp target done | D-331 |
| 2026-11-15 | 8.1 campaign workflow and coverage gate built | D-331 |
| 2026-11-15 | 8.1 campaign: first run green, coverage recorded | D-331 |
| 2026-11-15 | 8.4 format frozen at 1.0, extension columns | D-335, D-336 |
| 2026-11-15 | 8.2 threat model and unsafe audit done | D-333 |
| 2026-11-15 | 8.2 server limits done | D-334 |
| 2026-11-15 | 8.4 gains the format freeze, Q-028 | the format is still major 0 and D-029's benchmarks are not all ticked |
| 2026-11-15 | 8.3: Instruments in place of `samply`; profiles, two changes and the scaling gate done | D-338 |
| 2026-11-15 | 8.3: suite C built (`tools/competitive`, `competitive.yml`); the parse change re-measured in its final form | D-340 |
| 2026-11-15 | 8.4: the API review done, two parameters renamed; the CHANGELOG's 1.0 section | D-341 |
| 2026-11-15 | 8.4: the docs audit done; the guides spell each language's own names | D-343 |
| 2026-11-15 | 8.5: populations shipped before the data release (the schema promised them; no pack had them) | D-351 |
| 2026-11-15 | 8.5: the data release split into the pre-release (done, data-2026.11.15) and the catalog (after the key ceremony) | D-247 |
| 2026-11-16 | 8.6: v1.0.0 on GitHub first, the registries after the maintainer's test | D-352 |
| 2026-11-17 | 8.5 registry accounts and 8.6 registries checked: 1.0.1 by hand, 1.1.0 from CI | The release replaced the planned tag-republish mode |

| 2026-11-17 | 8.5b: packs at run time (TG-24 to TG-26), the browser store's research | D-362 |
| 2026-11-17 | 8.5b: TG-26 by a profile rather than `extract` | The search keys need the forge's keying rules |
