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
- [ ] `fuzz-campaign.yml`: weekly and by dispatch, 3 h per target, a job each, with coverage
  reports as artifacts. Gate: every section kind's decoder reached, the coverage recorded here
  - **Built (2026-11-15):** the workflow ([ci.md](../../06-engineering/ci.md#fuzzing-campaign))
    and its gate `cargo xtask fuzz-coverage`. The gate fails on a decoder no target reached and
    on a kind with no decoder entry (both planted). It read a real `llvm-cov` export locally
    (the `sections` target on whole-pack seeds, rightly red: that harness takes payloads)
  - **Open:** the first run's coverage, recorded here

### 8.2 Security review

- [ ] `docs/06-engineering/threat-model.md`: each trust boundary of `SECURITY.md`'s scope, what
  an attacker controls, what must hold, and the test or gate that shows it
- [ ] `cargo xtask unsafe-audit`: every `unsafe` block with its `SAFETY:` argument and the test
  that reaches it, written to the threat model; fails on a block without one (planted)
- [ ] Resource limits on every server route and MCP tool (body, result count, time), a test per
  limit
- [ ] `zizmor` on every workflow in fast-check, findings fixed or each one justified
- [ ] OpenSSF Scorecard weekly on `main`; the checks only the maintainer can satisfy listed
  for them
- [ ] The review's findings fixed, each with its test; `SECURITY.md` states the 1.0 support
  policy

### 8.3 Performance pass

- [ ] A `samply` profile of each Q and L suite on Nepal's main pack and the world baseline; the
  three largest self-time items per suite recorded in `docs/02-research/benches/phase-8.md`
- [ ] Each change justified by its profile and landed with its instruction count and
  wall-clock before and after
- [ ] The scaling gate (`cargo xtask scaling`): throughput at 1, 2, 4 and 8 threads on one
  `Shared` engine, recorded for the measured page
- [ ] Suite C, the competitive report (`competitive.yml`, by dispatch on GitHub runners):
  Nominatim and Photon on Nepal's extract, `tzf-rs` and a GeoNames-backed library, on the
  golden queries. Medians of repeated runs, with each rival's version and configuration

### 8.4 Docs complete and the API frozen

- [ ] The 1.0 API review: every IDL item read once for name, shape and error; changes made
  before the freeze, each through the bindings' `--check`
- [ ] `docs/06-engineering/semver.md`: what 1.0 promises (Rust API, C ABI, pack format, catalog,
  HTTP and MCP), MSRV and deprecation
- [ ] `cargo-semver-checks` in `release.yml` from the first tag, beside the ABI gate (D-285)
- [ ] A docs audit: every guide, concept and reference page read against the code once; the
  1.0 section of `CHANGELOG.md`

### 8.5 The first data release

- [ ] Phase 3.7: the lite build from clean-room sources only, and Nepal's `ACCURACY.md` against
  held-out truth (Q-017)
- [ ] Phase 3.8: M3 met
- [ ] Nepal's main and lite packs as a data release, through the catalog, with their accuracy
  summary
- [ ] A dry run of `release.yml` end to end at the release commit
- [ ] The maintainer: the key ceremony (phase 4.3), the release environment and registry
  accounts (5.10c), Pages (`DOCS_DEPLOY`)

### 8.6 Exit review

- [ ] v1.0.0 tagged and published; installs from every registry checked
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
