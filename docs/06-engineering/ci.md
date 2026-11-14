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
| `catalog-timestamp` | daily, and by hand; off until `CATALOG_LIVE` is `true` | signs a 14-day catalog timestamp and pushes it to the `catalog` branch; fails 7 days before any expiry ([catalog keys](catalog-keys.md), D-163) | ≤ 20 min |
| `fast-check` | every push and PR | fmt, clippy, unit and property tests on fixtures, `check-docs`, `check-data` (country folders and the source lock), `check-workspace`; the same fmt, clippy and tests for the `forge/` workspace in its own job; `cargo deny` for both workspaces; typos; `tuf-oracle`: python-tuf rebuilds the TUF differential corpus and python-jsonschema checks the catalog schema, any change fails (D-161); `tough-oracle`: tough and our client judge 17 repositories written at run time (D-165) | ≤ 10 min |
| `counts` | push to `main` and PRs touching `crates/` | instruction counts of the hot paths under Callgrind (gungraun, `teigeodata-bench/benches/counts.rs`): a PR measures its base first and fails above +1% instructions on any benchmark | ≤ 40 min |
| `verify` | push to `main` | full tests on Linux x64/arm64, macOS arm64, Windows x64; doc tests; golden; parity (fast set) | ≤ 30 min |
| `bindings` | push to `main`, release | cross-compile matrix (Android ABIs, iOS slices, WASM variants, desktop targets), binding tests per language | ≤ 45 min |
| `c-package` | `v*` tag, push to `main` touching the C library or its packager, by hand | `cargo xtask package-c --smoke`, full and lite, on Linux x64/arm64 (glibc on Ubuntu 22.04; musl in Alpine 3.23, 5.8a), macOS arm64 (x64 cross-built), Windows x64/arm64; on a tag `cargo xtask abi --release` and the archives with SHA256SUMS on the release (D-286) | ≤ 40 min |
| `apple` | `v*` tag, push to `main` touching the Swift binding, the C library or its fixtures, by hand | `cargo xtask package-swift --test`: the XCFramework for iOS, the simulators and macOS, the Swift parity checks against it on macOS and an iOS simulator; on a tag the zip and its checksum on the release (D-287) | ≤ 50 min |
| `jvm` | `v*` tag, push to `main` touching the JVM binding, its API or fixtures, by hand | the JNI library on six desktop platforms, the JAR tested from itself on each OS that runs it, one JAR with all six; the AAR (NDK r30, 16 KB checked) and parity on API 24 and API 35 emulators through `app_process`; on a tag both on the release (D-288) | ≤ 45 min |
| `flutter` | push to `main` touching the Dart package, the C library or its API, by hand | `cargo xtask flutter-check`: the example's integration test in a Flutter app built with the package's hook, on macOS, the iOS simulator, Linux (Xvfb), Windows and an API 35 emulator (D-289) | ≤ 60 min |
| `python` | `v*` tag, push to `main` touching the Python binding, its API or fixtures, by hand | cibuildwheel: cp311-abi3 (tested on 3.11 to 3.15), cp315-abi3t and cp314t wheels (tested free-threaded) on Linux (manylinux, musllinux), macOS and Windows, x86_64 and arm64, each tested from the installed wheel; every wheel checked for `LICENSE` and `NOTICE`; on a tag the wheels on the release (D-290) | ≤ 90 min |
| `wasm` | `v*` tag, push to `main` touching the API, the runtime crates, the WebAssembly binding or its fixtures, by hand | `cargo xtask package-wasm --test`: the `full` and `lookup` packages, each module held to its gzip budget, parity from the package on Node.js, Bun and Deno; then from the built packages on Node.js 22, 24 and 26, in Chromium, Firefox and WebKit (Playwright) and in workerd (Miniflare); on a tag the packed packages on the release (D-293) | ≤ 45 min |
| `node` | `v*` tag, push to `main` touching the API, the engine, the Node.js binding or its fixtures, by hand | the addon built for eight platforms (Linux glibc and musl, macOS, Windows; x64 and arm64) and its parity test on Node.js 22; `cargo xtask package-node --addons`; then on each platform the packages installed as npm would and tested from the installed package on Node.js 26 (D-294); on a tag the packed packages on the release | ≤ 40 min |
| `go` | push to `main` touching the API, the engine, the C library, the Go binding or its fixtures, by hand | the C library built on each platform, then the Go parity suite through cgo and through purego (`CGO_ENABLED=0`) on Go 1.26, on Linux glibc and musl (Alpine), x64 and arm64, macOS arm64 and Windows x64 (purego only on Windows arm64); `fast-check` runs Go 1.27 with `-race`, `FuzzReader` for 20 s and the lite library (D-297) | ≤ 30 min |
| `dotnet` | push to `main` touching the API, the engine, the C library, the .NET binding or its fixtures, by hand | the C library built on each platform, then the .NET parity program under the JIT and as a NativeAOT executable, on `linux-x64`, `linux-arm64`, `linux-musl-x64` and `linux-musl-arm64` (Alpine), `osx-arm64`, `win-x64` and `win-arm64`; `dotnet pack` on Linux (D-298) | ≤ 40 min |
| `fast-check` (`windows` job) | push, PR | clippy and tests of `teigeodata-sync` and `teigeodata-cli` on `windows-latest` (D-190) | ≤ 25 min |
| `nightly` | schedule | fuzzing, Miri, sanitizers, mutants, coverage, differential, benchmarks against baselines | ≤ 6 h per job |
| `data-build` | weekly (Monday 03:41 UTC) and manual | one job per pack: `forge make --fetch`, then G-det (the build stage again, `cmp`), G-lic (`cargo xtask check-pack`, also on the baseline slice, D-145, and the world part, D-147), the world and locator gates
(`tests/world.rs` on the built files: cities under their country, the slice's IDs, the border
probes, `missing_packs`; D-147, D-149) and the golden gate (`pack-eval` with `S3_GATE`); reports as artifacts. Later: sharded tiles and a final verify-sign-publish job | per shard ≤ 3 h |
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
- A data build fetches immutable upstreams directly and moving ones from the content-addressed
  `sources` release (D-127), so a run never depends on today's upstream.
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
- **world-install**: the world-size install (D-188), in release: a 576 MiB pack whose
  `BLOCK_HASHES` spans two frames, installed, cut at every request and mid-NAMES, and resumed
  through `fetch::install`. About 1.2 GB of memory and 600 MB of disk, so it is `#[ignore]` in
  `cargo test`.
  The core property tests take their case counts from `crate::cases(n)`: `n` natively, a
  thousandth (at least 4) under Miri; libFuzzer covers the volume.

Locally: `cargo +nightly fuzz run <target> -- -max_total_time=30 -rss_limit_mb=1024`; the
seeded run is part of `cargo test -p teigeodata-format`.
Delete `fuzz/target` afterwards (about 0.5 GB).
