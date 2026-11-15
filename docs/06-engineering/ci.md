---
title: "CI"
status: draft
updated: 2026-11-15
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
| `fast-check` | every push and PR | fmt, clippy, unit and property tests on fixtures, `check-docs`, `check-data` (country folders and the source lock), `check-workspace`, `unsafe-audit --check` (D-333); `zizmor` 1.32.0 on every workflow (template injection, permissions, credentials left in checkouts, unpinned or impostor actions; a finding kept on purpose carries an inline `zizmor: ignore[...]` with its reason); the same fmt, clippy and tests for the `forge/` workspace in its own job; `cargo deny` for both workspaces; typos; `tuf-oracle`: python-tuf rebuilds the TUF differential corpus and python-jsonschema checks the catalog schema, any change fails (D-161); `tough-oracle`: tough and our client judge 17 repositories written at run time (D-165); the docs site's tours on the pinned public pack, their output checked (D-322): Python, Node.js, Go, Swift, Dart, Java, Kotlin, C and C++ in the bindings job, Rust and the CLI (with `tgd serve` asked over HTTP) in the `examples` job | ≤ 10 min |
| `counts` | push to `main` and PRs touching `crates/` | instruction counts of the hot paths under Callgrind (gungraun, `teigeodata-bench/benches/counts.rs`): a PR measures its base first and fails above +1% instructions on any benchmark | ≤ 40 min |
| `verify` | push to `main` | full tests on Linux x64/arm64, macOS arm64, Windows x64; doc tests; golden; parity (fast set) | ≤ 30 min |
| `bindings` | push to `main`, release | cross-compile matrix (Android ABIs, iOS slices, WASM variants, desktop targets), binding tests per language | ≤ 45 min |
| `c-package` | called by `release`, push to `main` touching the C library or its packager, by hand | `cargo xtask package-c --smoke`, full and lite, on Linux x64/arm64 (glibc on Ubuntu 22.04; musl in Alpine 3.23, 5.8a), macOS arm64 (x64 cross-built), Windows x64/arm64 (D-286); the library alone for Dart and Go on those eight and, in the `mobile` job, on three Android targets (NDK r28+, API 24, 16 KB checked) and three iOS targets (D-304) | ≤ 40 min |
| `apple` | called by `release`, push to `main` touching the Swift binding, the C library or its fixtures, by hand | `cargo xtask package-swift --test`: the XCFramework for iOS, the simulators and macOS, the Swift parity checks against it on macOS and an iOS simulator; the zip and its checksum as an artifact for a release (D-287) | ≤ 50 min |
| `jvm` | called by `release`, push to `main` touching the JVM binding, its API or fixtures, by hand | the JNI library on six desktop platforms, the JAR tested from itself on each OS that runs it, one JAR with all six; the AAR (NDK r30, 16 KB checked) and parity on API 24 and API 35 emulators through `app_process`; both as artifacts for a release (D-288) | ≤ 45 min |
| `flutter` | push to `main` touching the Dart package, the C library or its API, by hand | `cargo xtask flutter-check`: the example's integration test in a Flutter app built with the package's hook, on macOS, the iOS simulator, Linux (Xvfb), Windows and an API 35 emulator (D-289) | ≤ 60 min |
| `python` | called by `release`, push to `main` touching the Python binding, its API or fixtures, by hand | cibuildwheel: cp311-abi3 (tested on 3.11 to 3.15), cp315-abi3t and cp314t wheels (tested free-threaded) on Linux (manylinux, musllinux), macOS and Windows, x86_64 and arm64, each tested from the installed wheel; every wheel checked for `LICENSE` and `NOTICE`; the wheels as artifacts for a release (D-290) | ≤ 90 min |
| `wasm` | called by `release`, push to `main` touching the API, the runtime crates, the WebAssembly binding or its fixtures, by hand | `cargo xtask package-wasm --test`: the `full` and `lookup` packages, each module held to its gzip budget, parity from the package on Node.js, Bun and Deno, and the docs site's WebAssembly tour (D-322); then from the built packages on Node.js 22, 24 and 26, in Chromium, Firefox and WebKit (Playwright) and in workerd (Miniflare); the packages as an artifact for a release (D-293) | ≤ 45 min |
| `docs` | push and PR touching the site, the IDL, the runtime crates, the WebAssembly binding or the playground's packager | the docs site: the guides against their generator, `npm ci`, `cargo xtask playground --fetch` (the full WebAssembly package from the commit, the pinned pack, MapLibre's worker; the tools from `.github/actions/wasm-tools`, shared with `wasm`), the static export, a type check, every internal link, the search index within its budget (gzip at most 300 KiB), and the playground in headless Chromium against the tours' answers, failing on any request to another origin (D-328); on `main`, deploys to GitHub Pages once `DOCS_DEPLOY` is `true` | ≤ 40 min |
| `node` | called by `release`, push to `main` touching the API, the engine, the Node.js binding or its fixtures, by hand | the addon built for eight platforms (Linux glibc and musl, macOS, Windows; x64 and arm64) and its parity test on Node.js 22; `cargo xtask package-node --addons`; then on each platform the packages installed as npm would and tested from the installed package on Node.js 26 (D-294); the packages as an artifact for a release | ≤ 40 min |
| `go` | push to `main` touching the API, the engine, the C library, the Go binding or its fixtures, by hand | the C library built on each platform, then the Go parity suite through cgo and through purego (`CGO_ENABLED=0`) on Go 1.26, on Linux glibc and musl (Alpine), x64 and arm64, macOS arm64 and Windows x64 (purego only on Windows arm64); `fast-check` runs Go 1.27 with `-race`, `FuzzReader` for 20 s and the lite library (D-297) | ≤ 30 min |
| `dotnet` | push to `main` touching the API, the engine, the C library, the .NET binding or its fixtures, by hand | the C library built on each platform, then the .NET parity program under the JIT and as a NativeAOT executable, on `linux-x64`, `linux-arm64`, `linux-musl-x64` and `linux-musl-arm64` (Alpine), `osx-arm64`, `win-x64` and `win-arm64`; on Linux x64 `cargo xtask package-dotnet --test` restores the `.nupkg` into a fresh project and runs the parity program from it, under the JIT and NativeAOT (D-298), and the docs site's .NET tour (D-322) | ≤ 40 min |
| `parity-mutants` | weekly (Monday 05:23 UTC), push to `main` touching `parity.json`, a parity suite or the task, by hand | the parity gate (5.9, D-300): `cargo xtask parity-mutants --lang <l>`, one job per language (Python, Node.js, Go, JVM, Swift, Dart, C, C++); every value of `bindings/parity.json` is planted in turn and the suite must fail on each; `fast-check` runs `--sample` (the first value of each section) in every language | ≤ 60 min |
| `fast-check` (`windows` job) | push, PR | clippy and tests of `teigeodata-sync` and `teigeodata-cli` on `windows-latest` (D-190) | ≤ 25 min |
| `nightly` | schedule | fuzzing, Miri, sanitizers, mutants, coverage, differential, benchmarks against baselines | ≤ 6 h per job |
| `scorecard` | weekly (Monday 04:29 UTC), by hand | OpenSSF Scorecard 2.5.0 on `main`: results published to the OpenSSF API (the public badge) and uploaded to code scanning. Checks only the maintainer can satisfy: Branch-Protection (rules on `main`), Code-Review (reviewed PRs once contributors arrive), CII-Best-Practices (registering the project), Signed-Releases (the first release, phase 8.5) | ≤ 20 min |
| `fuzz-campaign` | weekly (Saturday 06:13 UTC), by hand (`seconds`, at most 5 h) | every libFuzzer target for 3 h, one job each, continuing the nightly corpus; the minimised corpus replayed under `cargo fuzz coverage`; the coverage gate (`cargo xtask fuzz-coverage`): every section kind's decoder reached by some target, the report in the job summary and as an artifact (D-331, [below](#fuzzing-campaign)) | ≤ 340 min per job |
| `data-build` | weekly (Monday 03:41 UTC) and manual | one job per pack: `forge make --fetch`, then G-det (the build stage again, `cmp`), G-lic (`cargo xtask check-pack`, also on the baseline slice, D-145, and the world part, D-147), the world and locator gates
(`tests/world.rs` on the built files: cities under their country, the slice's IDs, the border
probes, `missing_packs`; D-147, D-149) and the golden gate (`pack-eval` with `S3_GATE`); reports as artifacts. Later: sharded tiles and a final verify-sign-publish job | per shard ≤ 3 h |
| `release` | by hand (`dry_run` by default) | the ABI release gate (a warning in a dry run); calls `c-package`, `apple`, `jvm`, `python`, `node` and `wasm`; gathers their files with a CycloneDX SBOM per crate and `SHA256SUMS`, attests build provenance and the SBOMs (Sigstore) and verifies every attestation (D-302); makes the release commit (`cargo xtask release-pin`) and runs each registry's check: Swift, Go, `dart pub publish --dry-run`, `npm publish --dry-run`, `twine check --strict`, the Maven bundle, `cargo publish --workspace --dry-run` (D-303, D-304); a real run tags `vX.Y.Z`, drafts the release, uploads and publishes it | ≤ 100 min |
| `pub` | a `v*` tag pushed by `release` (its deploy key) | the tag checked against the pubspec and the pinned hashes; `dart pub publish` through pub.dev's OIDC (D-305) | ≤ 15 min |
| `docs` | push to `main` and PRs touching `site/` or the IDL, by hand | the language guides checked against their generator (`site/scripts/guides.py --check`); the Fumadocs site built as a static export, type-checked, every internal link and fragment checked (`site/scripts/check-links.mjs`); deployed to GitHub Pages from `main` once Pages is enabled and `DOCS_DEPLOY` is `true` (phase 7) | ≤ 20 min |

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
  `teigeodata_format::fuzz`; `transport_table`, `transport_asset`, `transport_frame` and `trust` (the TUF update chain and
  the catalog on a repository with one role attacked and signed again, D-331) in
  `teigeodata_sync::fuzz` (D-112, `crates/teigeodata-sync/tests/fuzz.rs`); `server_query` in
  `teigeodata_server::fuzz` (D-313, `crates/teigeodata-server/tests/fuzz.rs`, run by fast-check's
  `check` job) and `mcp` there too (D-331): every MCP tool with arguments built from its own
  input schema, and resource reads, failing on an `internal` answer; `engine` in `teigeodata_engine::fuzz` (D-331, `crates/teigeodata-engine/tests/fuzz.rs`):
  every query on a seed pack with one or two sections edited and every hash right, the seeds
  embedded from `bindings/fixtures/` and `fuzz/seeds/`; `ffi` in `teigeodata_ffi::fuzz` (D-331,
  `crates/teigeodata-ffi/tests/fuzz.rs`): every `Engine` function of the C ABI with fuzzed
  arguments, a test failing when the ABI gains one it does not call. A crash input is kept as a test in the
  crate's `found_by_libfuzzer`. The corpus of each target is restored from the Actions cache, minimised
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

## Fuzzing campaign

`.github/workflows/fuzz-campaign.yml` (phase 8.1, D-331) runs each nightly target for 3 hours,
weekly on Saturday at 06:13 UTC (after the nightly has finished) and on demand with `seconds`
as an input (at most 18,000, so fuzzing, minimising and the coverage replay fit in the 6-hour
job limit). It runs on GitHub's runners, free for a public repository; a long run never uses
this machine (AGENTS.md §7).

- **Corpus:** the same cache keys as the nightly (`fuzz-corpus-<target>-`). The campaign starts
  from the newest corpus either workflow saved and saves its own, minimised, which the next
  nightly continues.
- **Coverage:** the minimised corpus is replayed by `cargo fuzz coverage`. `llvm-cov export
  -summary-only` writes `<target>.json` and `llvm-cov report` writes a per-file text report;
  both, with libFuzzer's final stats, are the artifact `fuzz-coverage-<target>`.
- **The gate** (`gate` job, `cargo xtask fuzz-coverage`): every section kind's decoder module in
  `crates/teigeodata-format/src/` has covered lines in at least one target. It reads the kinds
  from the reader (`Kind::name`), so a kind added without a decoder entry fails its test, and it
  fails on a decoder no target reached (both planted). The report (each kind's best target with
  its line and function coverage, then each target's totals) is the job summary and the
  artifact `fuzz-campaign-report`. Coverage is per file: kinds that share a layout (SEARCH and
  SUBSTRING, REGION and ADMIN_REGION, ARCS and ARCS_OVERVIEW, DEM and DEM_COARSE) share a row's
  figures. The gate runs when a target crashed too, so the other targets' report is written.
- **A crash** fails its job and uploads the input as `fuzz-artifacts-<target>`; it becomes a
  `found_by_libfuzzer` regression in the crate's `tests/fuzz.rs`.

Locally (seeds or a corpus): `cargo +nightly fuzz coverage <target> <dir>`, then `llvm-cov
export -summary-only` into a directory and `cargo xtask fuzz-coverage <dir>`. Delete
`fuzz/target` and `fuzz/coverage` afterwards (about 0.7 GB).

Locally: `cargo +nightly fuzz run <target> -- -max_total_time=30 -rss_limit_mb=1024`; the
seeded run is part of `cargo test -p teigeodata-format`.
Delete `fuzz/target` afterwards (about 0.5 GB).
