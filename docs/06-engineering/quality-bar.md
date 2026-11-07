---
title: "Quality bar"
status: accepted
updated: 2026-11-06
---

# Quality bar

Binding ([ADR-0018](../03-decisions/adr/ADR-0018-quality-bar.md)). A step is done only when
every gate that applies to it is green. Each gate is a `cargo xtask` command; CI runs the fast
ones on every pull request, the slow ones nightly, and all of them before a release.

## Code

| Gate | What it checks | When |
|---|---|---|
| fmt | `cargo fmt --check` | PR |
| lints | workspace lints as errors: pedantic clippy, no `unwrap`/`expect`/`panic`/indexing in libraries, `unsafe_code = forbid` outside the audited module, `missing_docs` | PR |
| deny | licences, advisories, bans, sources (`cargo deny`) | PR |
| dependency `unsafe` | our crates stay `forbid(unsafe_code)`; a dependency's internal `unsafe` is accepted when it is widely used, fuzzed upstream and replaces a large job (today `blake3` and `ed25519-dalek` SIMD and field arithmetic, and `ruzstd` behind the opt-in `zstd` feature, D-079). Untrusted input reaching it is bounded first (lengths, window) and covered by our fuzz targets | PR |
| vet | audited dependencies (`cargo vet`) | release |
| typos | spelling in code and docs | PR |
| docs | `cargo xtask check-docs`; rustdoc with no broken links; doc examples run | PR |
| country data | `cargo xtask check-data`: every `data/countries/<cc>/` profile (admin ladder, unit types, source codes) and lexicon (IDs, NFC, any ISO 15924 script, variant collisions, evidence, dated maintainer rulings) | PR |
| msrv | builds on the recorded MSRV | nightly |

## Correctness

| Gate | What it checks | When |
|---|---|---|
| unit | behaviour, including edge cases (poles, the antimeridian, empty packs, holes) | PR |
| property | invariants (encode/decode round-trips; merge is order-independent) with `proptest` | PR |
| golden | fixed queries and fixed answers on fixture packs | PR |
| differential | against reference implementations: point-in-polygon (GEOS-equivalent), timezone (reference tz lookups), compression round-trips | nightly |
| metamorphic | splitting, reordering or compacting packs does not change answers | nightly |
| parity | the same golden queries in every binding give identical answers | PR (fast set), nightly (full) |
| fuzz | every decoder and parser (`cargo fuzz`), with a corpus kept in the repo | nightly |
| miri, sanitizers | the zero-copy module and the FFI under Miri and AddressSanitizer | nightly |
| mutants | `cargo mutants` on changed code; survivors reviewed | nightly |
| coverage | line and branch coverage floors that only rise | nightly |
| no-panic boundary | the FFI never unwinds; errors are values | PR |

## Data

| Gate | What it checks | When |
|---|---|---|
| accuracy | horizontal and vertical P50/P90 per country and feature class against held-out ground truth; writes `ACCURACY.md` | data build |
| topology | no gaps or overlaps between siblings at any level of detail; children inside parents; points inside their polygons | data build |
| duplicates | residual duplicate rate per country | data build |
| search | recall@1 and @5 and MRR on golden queries, mixed scripts included | data build, PR touching search |
| licence | lite builds contain no ODbL/share-alike/NC provenance; `ATTRIBUTION.md` matches the sources | data build |
| id stability | no ID reused; every removal is a tombstone or a redirect; release diff report | data build |
| reproducibility | the same `sources.lock` gives byte-identical packs | data build |
| point in polygon | every place lies in exactly one region per covered admin level; unassigned places listed and capped | data build |
| conflation | precision ≥ 0.98 and recall ≥ 0.95 on labelled pairs (targets set from spike S5) | data build |
| corrections | no stale correction unreviewed for longer than one release ([ADR-0036](../03-decisions/adr/ADR-0036-corrections-as-data.md)) | data build |
| budget | every forge stage within its declared disk and RSS budget; every CI shard at least 30 % under 6 h | data build |
| sync | the 15 trust and install gates of [ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md) (rollback, freeze, crash during install, resume against a host that ignores `If-Range`, …) | PR touching `sync` |

## Performance and resources

| Gate | What it checks | When |
|---|---|---|
| instructions | `gungraun` instruction counts of the Q and L suites within +1 % of the baseline ([benchmarks](benchmarks.md)) | PR touching hot paths |
| benchmarks | `criterion` wall-clock and every binding's harness against per-runner baselines; regressions with p < 0.01 open an issue; writes `BENCHMARKS.md` | nightly |
| allocations | counted allocations in hot paths asserted in tests (zero where designed) | PR |
| memory | peak memory and leak checks in Rust and in every binding | nightly |
| sizes | pack size per country and component; library size per variant and target; WASM size; writes `SIZES.md` | PR touching format or build |
| scaling | multi-threaded query throughput scales | nightly |

## Rules

- Every optimisation is preceded by a profile and followed by a measurement in the commit body.
- **A new gate is proven red** on a deliberate violation before it is trusted.
- Generated pages are never hand-edited.
