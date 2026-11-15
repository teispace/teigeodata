---
title: "Hardening and the v1.0.0 release"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-02-15
---

# Hardening and the v1.0.0 release

## 1. Question

Phase 8 ends at **M8 = v1.0.0**: a fuzzing campaign, a performance pass, a security review,
docs complete, and the first data release ([roadmap](../../07-planning/roadmap.md)). Phases 1 to 7
each fuzzed, benchmarked and documented their own part. This note asks what is still missing
for a 1.0 that strangers may put in front of untrusted input, depend on through semver, and read
the docs of without help. It also fixes the order of the work, what proves each part, and what
only the maintainer can do. It feeds [phase 8](../../07-planning/phases/phase-8.md) and D-331.

## 2. Summary and recommendation

- **The fuzzing campaign follows untrusted bytes to the code that answers queries.** The 12
  libFuzzer targets reach the pack container, the transport, the HTTP query parser and five of
  the 22 section kinds' decoders. **Nothing fuzzes the engine on a pack whose sections are
  wrong but correctly hashed.** That is the path `SECURITY.md` promises to keep safe ("packs
  from untrusted sources must never cause … panics … unbounded memory or CPU use"): a pack a
  user builds or downloads from anywhere that is not the signed catalog. The format crate's
  `check` proves a pack whole. The engine does not call it on open, because opening is O(head).
  So every query path must survive a section that `open` accepts but `check` would refuse.
  New targets, in this order:
  1. `engine`: a synthetic pack with every section kind (the engine tests' model, moved into
     the crate behind a `fuzz` feature). One section's bytes are mutated, the pack is rewritten
     by the writer so every hash is right, then it is opened with `Layer::from_bytes` and asked
     every query once, under a step budget. Property: no panic, no allocation over a bound,
     every answer or a typed error.
  2. `payloads`: every section kind's `open` and `check` (the five done today, plus the other
     17), and the property that a section `check` accepts never makes a query fail with
     anything but a typed error.
  3. `trust`: the TUF metadata parser and the catalog's `custom` fields
     (`sync::trust`). Its differential against python-tuf (D-161) runs on fixed cases and has
     never been fuzzed.
  4. `ffi`: the C ABI's string and buffer arguments (lengths, NUL bytes, invalid UTF-8, null
     pointers where documented as allowed), through the generated layer.
  5. `mcp`: JSON-RPC messages into the MCP server's request core (the HTTP query parser is
     fuzzed already; the MCP framing is not).
- **The campaign is bounded and asynchronous** (D-080). Nightly stays at 300 s per target. A
  weekly `campaign` dispatch runs each target for 3 h as its own GitHub job. That is free on a
  public repository and runs on none of this machine's cores. It writes `cargo fuzz coverage`
  reports as artifacts, and their summary lands in the phase file. **Gate:** every decoder of
  every section kind is reached, with its line coverage recorded. Every crash input becomes an
  inline regression in the crate's `tests/fuzz.rs` (`found_by_libfuzzer`, the convention the
  sync crate set and the format crate adopted on 2026-11-15).
- **The performance pass profiles before it optimises** (quality bar: "every optimisation is
  preceded by a profile"). Each Q and L suite runs on the Nepal main pack and the world
  baseline under `samply`; the three largest self-time items per suite are recorded; a change
  lands only with its instruction count (`counts.yml`) and wall-clock before and after. Two
  release gates are written in [benchmarks](../../06-engineering/benchmarks.md) but have never
  run: the **scaling** gate (query throughput with 1, 2, 4 and 8 threads on one `Shared`
  engine) and the **competitive report** (suite C). Both are built here. Suite C runs as a
  dispatch workflow on GitHub runners: Nominatim and Photon on Nepal's extract (about 100 MB of
  OSM data), `tzf-rs` and the GeoNames-backed library on the same golden queries. It never
  runs on this machine, whose disk and memory other projects share.
- **The security review is written down as a threat model, then checked surface by surface.**
  `docs/06-engineering/threat-model.md` lists each trust boundary of `SECURITY.md`'s scope
  (pack bytes, catalog and TUF metadata, the network, the C ABI, the servers, the build and
  release). For each it states what an attacker controls, what must hold, and the test or gate
  that shows it holds. The review then fills the gaps it finds:
  - an inventory of every `unsafe` block, each with its `SAFETY:` argument and the Miri or
    fuzz test that exercises it: 191 blocks today, 174 of them generated (`ffi/generated.rs`),
    10 in `ffi/abi.rs`, 3 in the engine's map, 4 in the sync crate's hole files;
  - resource limits on every server route and MCP tool (body size, result count, time),
    proved by a test per limit;
  - `zizmor` on every workflow, and the OpenSSF Scorecard action on `main`. The release
    pipeline already has SBOMs, SLSA provenance through Sigstore and pinned actions (D-302 to
    D-305).
- **Docs complete means checked, not read.** Every public item is documented (`missing_docs`
  is already a workspace lint, an error in CI). Every guide sample runs (D-322). `cargo doc`
  passes with `-D warnings` and every doc example runs. The site's links pass. Phase 8 adds
  four things:
  - a **1.0 API review**: every IDL item read once for name, shape and error, before semver
    freezes it;
  - a semver policy page;
  - a 1.0 `CHANGELOG.md` section;
  - a `cargo-semver-checks` gate from the first tag on, beside the C ABI gate D-285 already
    has (`cargo xtask abi --release`).
- **The first data release finishes Phase 3.** v1.0.0's data is Nepal's main and lite packs,
  signed through the catalog, with `ACCURACY.md`. That needs:
  - 3.7: the lite build from clean-room sources only, and Nepal's accuracy report against
    held-out truth, with its labels, Q-017;
  - 3.8: M3 met;
  - the key ceremony (phase 4, step 4.3) and the real release path (5.10c), both on
    the maintainer's accounts.

  Agents do everything up to the point where only the maintainer can act. A dry run of the
  whole release (`release.yml` with `dry-run`) proves the path before then.
- **M8 is met** when each of the five parts has its evidence in the phase file and the release
  has run for real. Agents can make everything except the last step ready on their own.

## 3. Findings

### Fuzzing today (measured 2026-11-15)

- **Targets:** 12 libFuzzer targets in `fuzz/fuzz_targets/`, one property function per target
  in the crates' `fuzz` modules (D-072). Nightly runs each for 300 s with `-max_len=4096` and
  `-rss_limit_mb=2048`, then minimises the corpus and caches it between runs.
- **Format crate:** `open`, `round_trip`, `mutate`, `manifest`, `sections` (PLACES, HIERARCHY,
  IDMAP, REDIRECTS, BLOBS), `bitpack`, `table`, `bits`.
- **Sync crate:** `transport_table`, `transport_asset`, `transport_frame`.
- **Server:** `server_query` (the HTTP query string).
- **Kinds without a fuzzed decoder:** the format's `check.rs` checks 22 section kinds. These 17
  are not in `sections`: ELEVATION, PLACE_TZ, ISO_CODES, NAMES, SEARCH, SUBSTRING, PLACE_INDEX,
  REGION, POSTAL, DEM, DEM_COARSE, COMPONENTS, ADMIN_REGION, ARCS, ARCS_OVERVIEW, POLYGONS,
  POLY_INDEX. Several are reached only through `check`, never through the engine's own reads.
- **Every failed nightly is accounted for (2026-11-15):**
  - `transport_table` failed on 2026-11-09 and 2026-11-10 ("the plan fetches the wrong
    frames"): the model in the property was wrong, fixed in 6a3ff32;
  - `mutate` failed on 2026-11-14 ("a section kind was given twice"): the harness rebuilt a
    pack by kind and lost its parts, fixed in 685495a.

  Both were harness faults, not reader faults, and both inputs now replay on every test run.
  The `mutate` replay fails with 685495a reverted (planted). Nightly has passed on every other
  day since 2026-11-11.
- **Engine entry:** `Layer::from_bytes(bytes, LayerOptions)` opens a pack held in memory and
  reads O(head). Sections are hashed when first read, as `LayerOptions::verify` says. So a
  harness that rewrites the pack with the writer reaches every decoder with correct hashes. The
  engine's tests already build a pack with every kind (`tests/common::synthetic(n, seed)`,
  then `Model::encode`).
- **Miri** runs nightly over the format, core and sync tests, by group. It covers the
  `unsafe` of the sync crate's hole files and of the engine's mapping only where a test
  reaches them.

### Supply chain today

- **Workflows:** `cargo deny` runs on every push (advisories, licences, bans, sources). All
  workflows pin actions by commit and give each job only the permissions it needs.
- **Releases:** `release.yml` writes CycloneDX SBOMs of every crate (`cargo-cyclonedx`
  0.5.10) and attests every file with SLSA build provenance (`attest-build-provenance` v4.2.2),
  verifying each attestation (D-302 to D-305).
- **Missing:** a scan of the workflows themselves (`zizmor`), a public score (OpenSSF
  Scorecard), and a semver check of the Rust API (the C ABI has one, D-285).

### Performance today

- **Gates that run:** instruction counts (`counts.yml`, the Q and L suites), allocations
  (asserted in tests), sizes (`cargo xtask sizes`), the phone-class bench (G-phone, D-235) and
  the server load gate (`cargo xtask load`, D-321).
- **Figures:** see the [measured page](../../../site/content/docs/data/measured.mdx) and
  [phase-4 benches](../benches/phase-4.md). Search p50 is 6.7 µs on Nepal (D-121); `countries_near`
  takes 11.9 µs (D-149); keystroke p99 is 0.2 ms on the phone class.
- **Written in [benchmarks](../../06-engineering/benchmarks.md) but never run:** scaling
  (multi-threaded throughput), the nightly wall-clock gate with an issue on regression, and the
  competitive suite C.

### This machine

- Shared, 16 GB, 10 cores; 25 GB free on 2026-11-15, under the 30 GB floor ([AGENTS.md](../../../AGENTS.md)
  §7). Long fuzzing, the competitive suite (Docker, Nominatim's import) and any hour-long run
  belong on GitHub's runners, which are free for this public repository.

## 4. Options compared

### How to fuzz the engine

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Raw bytes into `Layer::from_bytes` | Simplest | Every input fails the head hash; never reaches a decoder | — | — | Rejected (the `open` target already covers the head) |
| Mutate one section of a synthetic pack, rewrite with the writer, open and query | Reaches every decoder with right hashes; the fuzzer's bytes go where the bugs are; reuses the engine tests' model | The model moves from `tests/common` into the crate (behind `fuzz`) | Apache-2.0 (ours) | libfuzzer-sys 0.4.13 | **Chosen** |
| `arbitrary`-derived models of every section | Structure-aware from the start | A second model of the format to keep in step; harder to reach invalid-but-hashed bytes | MIT/Apache-2.0 | arbitrary 1.5.0 | Not now; for sections whose decoders the first never covers |
| Differential: the engine's answer against a naive reader of the same pack | Finds wrong answers, not only panics | A second reader; the metamorphic tests (`tests/metamorphic.rs`) already check answers on valid packs | — | — | Later, for search ranking only if the campaign finds nothing |

### Where long fuzzing runs

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Nightly 300 s per target (today) | Every night, cheap | Shallow for the new engine target | — | — | Kept |
| Weekly campaign, 3 h per target, one job each | Deep; free on a public repo; nothing runs here | A slow week finds a crash late, which D-080 accepts | — | GitHub-hosted, 6 h job limit | **Chosen** |
| OSS-Fuzz | Google's machines, continuous, ClusterFuzz triage | Admission is for widely used projects; apply after 1.0 when there are users | Apache-2.0 | — | After 1.0 (idea) |
| This machine overnight | No CI limits | Shared machine; other projects' builds; AGENTS.md §7 | — | — | Rejected |

### Supply-chain scanning

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `zizmor` on the workflows | Finds template injection, excess permissions, unpinned actions; offline | One more check | MIT | 1.32.0 (2026-11-02) | **Chosen** (fast-check) |
| OpenSSF Scorecard action | A public score users check; covers branch protection, signed releases | Some checks need the maintainer's settings | Apache-2.0 | v2.5.0 (2026-10-12) | **Chosen** (weekly) |
| CodeQL for Rust | Data-flow queries | Our `unsafe` is small and Miri-tested; slow | MIT (queries) | bundle v2.27.4 | Not now |
| `cargo-vet` | Audits every dependency | A large first audit for a solo maintainer | Apache-2.0/MIT | 0.10.3 | After 1.0; `cargo deny` covers advisories and sources |
| `cargo-semver-checks` | Catches a breaking change in the public Rust API | Only meaningful from the first published version | Apache-2.0/MIT | 0.51.0 (2026-10-03) | **Chosen** from v1.0.0 on |

### The competitive report

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| On GitHub runners, by dispatch, Nepal's extract | Free; reproducible; nothing local; one documented runner class | Shared runners vary, so we report medians of many runs and say so | — | — | **Chosen** |
| On this machine | One fixed machine | Nominatim's import needs Docker, gigabytes of disk and memory other projects use | — | — | Rejected |
| No competitive report | Less work | The benchmarks page promises one per release; a claim with no gate is not made | — | — | Rejected |

## 5. What we build ourselves, and why

- `teigeodata-engine` feature `fuzz` with `fuzz::engine(data)`. It builds the model from a
  seed, mutates one chosen section (byte flips, truncation, a column's length, a value set to
  a boundary, as `mutate` does for the head) and rewrites the pack. It opens the pack and calls
  each query once: `get`, `search`, `reverse`, `admin_at`, `tz_at`, `elevation_at`, `nearest`,
  `within`, `parent`, `ancestors`, `children`, `boundary`, `postal`, `by_iso` and
  `countries_near`. A step bound keeps it from looping. The same function runs under
  `cargo test` on seeded inputs and under libFuzzer, as D-072 set. **Proof:** a planted
  `indexing_slicing` panic in one section decoder is found within the nightly's 300 s.
- `fuzz_targets/{engine,payloads,trust,ffi,mcp}.rs`, and `nightly.yml`'s matrix grown to 17.
- `.github/workflows/fuzz-campaign.yml`: weekly and by dispatch, 3 h per target, coverage
  reports as artifacts.
- `docs/06-engineering/threat-model.md` and an `unsafe` inventory written by `cargo xtask
  unsafe-audit`. It lists every block with its file, line and `SAFETY:` comment, and fails on a
  block without one (planted).
- `xtask scaling` (the scaling gate) and `.github/workflows/competitive.yml` (suite C), each
  writing a record for the measured page (D-326).
- `docs/06-engineering/semver.md` and the `cargo-semver-checks` job in `release.yml`.

## 6. Impact on the plan

- **New:** [phase 8](../../07-planning/phases/phase-8.md); D-331; a roadmap changelog row.
- **Engineering docs:**
  - [ci.md](../../06-engineering/ci.md) gains the campaign, the Scorecard and the competitive
    workflow;
  - [quality-bar.md](../../06-engineering/quality-bar.md) gains the coverage gate, the `unsafe`
    audit and the semver gate;
  - [benchmarks.md](../../06-engineering/benchmarks.md) says where suite C runs.
- **Ordering:** Phase 3's 3.7 and 3.8 are on M8's path (the first data release), so they run
  inside Phase 8. Other work runs between them, because those steps wait on labels (Q-017).

## 7. Risks and open items

- **The engine target may find many faults at once.** Each one is fixed with its input kept,
  and a decoder that cannot be made total is checked on open (O(rows)), with the cost measured.
- **Nominatim's results depend on its configuration.** We publish the configuration and invite
  its maintainers to check, as [benchmarks](../../06-engineering/benchmarks.md) says.
- **Only the maintainer can do these** (each a step in the phase file): the key ceremony,
  the release environment's secrets and the registry accounts (5.10c), Pages
  (`DOCS_DEPLOY`), branch protection for Scorecard, and Q-009, Q-011 and Q-017's inputs.
- **Disk:** under the 30 GB floor on 2026-11-15. The engine target's build is light; anything
  heavy is cleaned straight after it.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| cargo-fuzz | 0.13.2 (2026-06-09) | 2026-11-15 | crates.io |
| libfuzzer-sys | 0.4.13 (2026-06-04) | 2026-11-15 | crates.io |
| arbitrary | 1.5.0 (2026-10-06) | 2026-11-15 | crates.io |
| cargo-llvm-cov | 0.9.1 (2026-09-06) | 2026-11-15 | crates.io |
| cargo-deny | 0.20.2 (2026-07-09) | 2026-11-15 | crates.io |
| cargo-audit | 0.23.0 (2026-11-10) | 2026-11-15 | crates.io |
| cargo-semver-checks | 0.51.0 (2026-10-03) | 2026-11-15 | crates.io |
| cargo-vet | 0.10.3 (2026-10-22) | 2026-11-15 | crates.io |
| cargo-cyclonedx | 0.5.10 (2026-11-06) | 2026-11-15 | crates.io |
| cargo-mutants | 27.1.0 (2026-06-02) | 2026-11-15 | crates.io |
| zizmor | 1.32.0 (2026-11-02) | 2026-11-15 | PyPI |
| ossf/scorecard-action | v2.5.0 (2026-10-12) | 2026-11-15 | GitHub releases |
| actions/attest-build-provenance | v4.2.2 (2026-08-06) | 2026-11-15 | GitHub releases |
| CodeQL bundle | v2.27.4 (2026-11-02) | 2026-11-15 | GitHub releases |

## 9. Sources

- The Rust Fuzz Book, "cargo-fuzz" and "Coverage", rust-fuzz.github.io, accessed 2026-11-15.
- libFuzzer, llvm.org/docs/LibFuzzer.html (options `-max_total_time`, `-rss_limit_mb`,
  `-max_len`), accessed 2026-11-15.
- zizmor documentation, docs.zizmor.sh (audits), accessed 2026-11-15.
- OpenSSF Scorecard, github.com/ossf/scorecard (checks and the action), accessed 2026-11-15.
- cargo-semver-checks, github.com/obi1kenobi/cargo-semver-checks, accessed 2026-11-15.
- OSS-Fuzz, google.github.io/oss-fuzz ("Accepting new projects"), accessed 2026-11-15.
- This repository: `nightly.yml`, `release.yml`, `deny.toml`, `crates/teigeodata-format/src/check.rs`,
  `crates/teigeodata-engine/src/layer.rs`, nightly runs 42596616936, 42794682758 and 43578466577.
