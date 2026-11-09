---
title: "CI"
status: draft
updated: 2026-11-09
---

# CI

The repository is public, so GitHub Actions is free and runs all CI
([ADR-0019](../03-decisions/adr/ADR-0019-ci.md)). `act` is not used for this repository.
Runner specs and limits are recorded in the [bindings and MCP research](../02-research/notes/bindings-and-mcp.md)
and checked again before each workflow change.

## Workflows

| Workflow | Trigger | Jobs | Budget |
|---|---|---|---|
| `fast-check` | every push and PR | fmt, clippy, unit and property tests on fixtures, `check-docs`, `check-data` (country folders and the source lock), `check-workspace`; the same fmt, clippy and tests for the `forge/` workspace in its own job; `cargo deny` for both workspaces; typos | ≤ 10 min |
| `counts` | push to `main` and PRs touching `crates/` | instruction counts of the hot paths under Callgrind (gungraun, `teigeodata-bench/benches/counts.rs`): a PR measures its base first and fails above +1% instructions on any benchmark | ≤ 40 min |
| `verify` | push to `main` | full tests on Linux x64/arm64, macOS arm64, Windows x64; doc tests; golden; parity (fast set) | ≤ 30 min |
| `bindings` | push to `main`, release | cross-compile matrix (Android ABIs, iOS slices, WASM variants, desktop targets), binding tests per language | ≤ 45 min |
| `nightly` | schedule | fuzzing, Miri, sanitizers, mutants, coverage, differential, benchmarks against baselines | ≤ 6 h per job |
| `data-build` | schedule, manual | sharded matrix: one job per pack or tile; content-addressed source cache; a final verify-sign-publish job | per shard ≤ 6 h |
| `release` | tag | crates.io, npm, PyPI, pub.dev, Maven Central, NuGet, SwiftPM, CLI binaries; signing, SBOM, provenance attestations | |
| `docs` | push to `main` | the Fumadocs site with generated pages, to GitHub Pages | ≤ 10 min |

Workflows are added when the code they test exists. `fast-check` exists from Phase 0; `nightly`
and `counts` from Phase 1.

**Counts, not wall clock, gate PRs:** this machine and shared runners vary up to 2× with load,
while instruction counts are the same on every run of the same code. The `counts` bench builds on
every host (so `cargo clippy --all-targets` checks it), but runs only on Linux, where Valgrind is;
locally: `cargo bench -p teigeodata-bench --bench counts` on Linux with `gungraun-runner` 0.20.0.
The criterion suites stay for wall-clock numbers on the results pages.

## Rules

- Actions are pinned by commit SHA; Renovate updates them.
- Least-privilege `permissions:` per workflow; `pull_request_target` is not used with untrusted code.
- Caches are keyed by lockfiles; large data intermediates go to release assets, not the Actions cache.
- Pushes are batched; a PR body says which checks ran.

## Nightly

`.github/workflows/nightly.yml` runs at 02:17 UTC and on demand (`workflow_dispatch`, with the
fuzzing time per target as an input).

- **fuzz**: one job per libFuzzer target in `fuzz/` (its own Cargo workspace), 300 s each by
  default, inputs up to 4 KiB. The targets call the same `fn(&[u8])` harnesses that
  `cargo test` runs on seeded inputs on every PR (D-072, `crates/teigeodata-format/tests/fuzz.rs`):
  `open`, `round_trip`, `mutate`, `manifest`, `sections`, `bitpack`, `table` and `bits`, all in
  `teigeodata_format::fuzz`; `transport_table`, `transport_asset` and `transport_frame` in
  `teigeodata_sync::fuzz` (D-112, `crates/teigeodata-sync/tests/fuzz.rs`). The corpus of each target is restored from the Actions cache, minimised
  with `cargo fuzz cmin` and saved again; a crash fails the job and uploads its input.
- **miri**: ten parallel jobs under `cargo miri nextest` (core lib; format lib in three
  parts; format `open`; format `fuzz` in four parts and sync `fuzz`, one seeded input per harness,
  `TG_FUZZ_ITERS=1`), 90 minutes each. Nextest runs every test in its own process with a
  40-minute limit (`.config/nextest.toml`, profile `miri`), so a slow test is named in the log;
  with `cargo miri test` in one process, `format-lib` and `format-fuzz` ran into the old 5-hour
  budget every night without saying which test was slow (2026-11-08). Under nextest the whole
  Miri set ran in at most 6 minutes per job (run 42559100430, 2026-11-09): one process per
  test avoids Miri interleaving every test's threads in one interpreter. Miri runs 10³ to
  10⁶ times slower than native (most for hashing and signatures), so tests shrink under `cfg(miri)`: the hashing and search
  tests take fewer lengths and smaller key sets, and the harnesses take smaller limits
  (`fuzz::limit`). On an Apple silicon Mac, run Miri with `RUSTFLAGS='--cfg sha2_backend="soft"'`:
  `sha2` calls an aarch64 crypto intrinsic that Miri does not support (cross-compiling to x86
  Linux fails on the C `zstd` dev-dependency). Miri's isolation hides the host environment,
  so the job forwards `TG_FUZZ_ITERS` with `MIRIFLAGS=-Zmiri-env-forward=TG_FUZZ_ITERS`, and the
  `fuzz` test defaults to one input under Miri anyway: unforwarded, every harness ran its
  4,000 native inputs and the job hit its 5-hour budget (found 2026-11-08; one input takes
  0.03 s for the bitmap harness, 4,000 took 21 minutes). Nightly results do not gate work: a
  failure is fixed when it is found.
  The core property tests take their case counts from `crate::cases(n)`: `n` natively, a
  thousandth (at least 4) under Miri; libFuzzer covers the volume.

Locally: `cargo +nightly fuzz run <target> -- -max_total_time=30 -rss_limit_mb=1024`; the
seeded run is part of `cargo test -p teigeodata-format`.
Delete `fuzz/target` afterwards (about 0.5 GB).
