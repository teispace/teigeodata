---
title: "Benchmarking: harnesses, methodology, CI and bindings"
status: research
updated: 2026-11-06
owner: maintainer
revisit: 2027-02-06
---

# Benchmarking: harnesses, methodology, CI and bindings

## 1. Question

How do we benchmark teigeodata so every speed, size and memory claim is reproducible? Three
things must hold together:

- every layer is covered, from the Rust core through the C ABI and each binding to a full app;
- many cases are covered (data sets, query distributions, pack sizes, cache states, platforms);
- CI catches regressions on a public repository with noisy hosted runners and no money for
  dedicated machines.

## 2. Summary and recommendation

- **One workload definition, many harnesses.** Workloads (query sets with fixed seeds, packs,
  operations) live in one crate, `teigeodata-bench`, and in checked-in query files under
  `bench/queries/`. Every harness and every binding runs the same files, so the numbers line up
  and FFI overhead is a subtraction: binding minus Rust.
- **Rust core:** use two harnesses side by side.
  - `gungraun` 0.20.0 (Callgrind/Cachegrind/DHAT): instruction counts, cache misses and heap
    use. These numbers are deterministic on hosted runners, so they are the **PR gate**.
  - `criterion` 0.8.2: wall-clock reports with confidence intervals. Run nightly and locally.
  - `tango-bench` 0.9.0 (paired A/B in one process) is for the research A/B comparisons that
    the format note asks for (B1–B14).
  - `divan` is not used: its last release is 0.1.21 from 2025-04.
- **End to end:** `hyperfine` 2.0.0 for the CLI (`tgd`): cold and warm open, install, build.
- **Bindings:** each language's standard harness, so its users trust the numbers:
  - Google Benchmark 1.10.0 for C and C++;
  - `pyperf` 2.10.0 for Python;
  - `mitata` 1.0.34 for Node, and for WASM inside Chrome, Firefox and WebKit through Playwright
    1.65.0;
  - JMH 1.37 for the JVM;
  - `androidx.benchmark` 1.5.0 on Android devices;
  - `package-benchmark` 1.38.1 for Swift;
  - BenchmarkDotNet 0.15.8 for .NET;
  - Go `testing.B` with `b.Loop` (Go 1.27.3);
  - `benchmark_harness` 2.4.0 for Dart; Flutter `integration_test` timeline on devices.
- **Results:** own versioned JSON schema (`teigeodata/bench/v1`) with machine metadata. An
  `xtask bench` command runs the suites and an `xtask bench-report` command writes
  `BENCHMARKS.md`.
  - Baselines are stored per runner class. History lives in a `bench-history` branch.
  - CodSpeed (free for open source, Walltime on bare-metal ARM64 "macro runners", 600
    min/month) and Bencher (open source, t-test thresholds) are evaluated in Phase 1 as an
    optional dashboard. Neither is the source of truth.
- **Gates:**
  - PR: instruction counts must stay within +1 % on the hot paths (tuned once measured), and
    allocation counts must hold (zero where designed).
  - Nightly: wall-clock regressions flagged with a Welch t-test across runs.
  - Release: the full matrix, including devices.

## 3. Findings

- **Noise on hosted runners.** Wall-clock timing on shared GitHub runners varies because of
  frequency scaling, neighbours and differing CPUs behind the same label. Two reports from
  practice:
  - CodSpeed saw "regressions" in unrelated PRs caused by being given a different CPU.
  - Rustls gates its PRs on Cachegrind instruction counts instead, minus a no-op baseline, and
    reports very low noise.

  Instruction counts do not need hardware performance counters, so they work on hosted runners.
  They are a proxy, though: +5 % instructions is not +5 % time, so wall-clock still matters and
  is reported separately.
- **`gungraun`** (formerly `iai-callgrind`, 0.16.1 was its last release under that name) runs
  each benchmark once under Valgrind and reports instruction, cache and heap differences against
  the previous run. Valgrind runs on Linux only (x86-64 and aarch64), which is enough for a gate.
- **`criterion` 0.8.2** (2026-02) is the maintained standard for statistics. Its compat crate
  `codspeed-criterion-compat` 6.0.0 lets the same benchmarks run on CodSpeed later without
  rewriting them.
- **`tango-bench`** alternates the baseline and the candidate in one process, which cancels most
  machine noise. That suits A/B decisions such as "FSST against zstd blocks".
- **Free arm64 Linux runners** (`ubuntu-24.04-arm`) exist for public repositories. macOS and
  Windows runners are also free for public repositories but noisy; they are used for
  correctness and coarse numbers only.
- **Mobile numbers need real devices.** `androidx.benchmark` locks clocks on rooted devices and
  warns on emulators. On iOS, `XCTest` `measure` blocks or `package-benchmark` on a device are
  the options. CI emulators are useful for "does it run", not for timing. Device runs are done
  by hand at each release, on a documented device list, until a device farm is affordable.
- **Spike S1 and S2 already showed the traps:** timings taken while the laptop's load average
  was about 5, and a per-query allocation (a `Vec` clone) that inflated S2 lookups. Both are
  exactly what the allocation gate and the instruction-count gate catch.

## 4. Options compared

| Option | Strength | Weakness | Decision |
|---|---|---|---|
| `criterion` 0.8.2 | statistics, HTML reports, CodSpeed compat | wall-clock: noisy in CI | **use** (nightly, local) |
| `gungraun` 0.20.0 | deterministic counts, cache and heap data | Linux and Valgrind only; a proxy for time | **use** (PR gate) |
| `tango-bench` 0.9.0 | paired A/B cancels noise | younger, smaller | **use** (research A/B) |
| `divan` 0.1.21 | ergonomic, allocation counting | no release since 2025-04 | not used |
| CodSpeed | free for OSS, bare-metal Walltime runners, PR comments | hosted service, minute quota, public repos need a runner-group setting | evaluate in Phase 1 |
| Bencher | open source, self-hostable, t-test thresholds | another service to run | evaluate in Phase 1 |
| `benchmark-action/github-action-benchmark` 1.22.2 | simple history on GitHub Pages | crude thresholds | fallback |
| Own JSON + `xtask bench-report` | no dependency, fits `BENCHMARKS.md` | we write it | **use** (source of truth) |

## 5. What we build ourselves, and why

- **`teigeodata-bench` workloads and query files.** Every harness runs the same inputs, so
  bindings and competitors can be compared directly.
- **The results schema and report generator**, because the generated pages (`BENCHMARKS.md`,
  `SIZES.md`) are part of the quality bar.
- **A network shaper for sync benchmarks:** a local HTTP server with latency, bandwidth, drop and
  "ignores `If-Range`" modes, matching what the offline-loading note measured on GitHub.
  Toxiproxy v2.13.0 is the alternative for the latency part.

## 6. Impact on the plan

- New page: [Benchmarks](../../06-engineering/benchmarks.md), the living catalogue of suites,
  cases, platforms, bindings and gates.
- [The quality bar](../../06-engineering/quality-bar.md): the benchmark gate is split into the
  instruction-count PR gate and the nightly wall-clock gate.
- Phase 1 builds `teigeodata-bench`, `xtask bench` and the gungraun gate before the first hot
  path ([ADR-0018](../../03-decisions/adr/ADR-0018-quality-bar.md) already requires that test
  infrastructure comes first).
- Phase 0 spikes stay as they are: indicative numbers, clearly labelled.

## 7. Risks and open items

- The instruction-count threshold (+1 %) is a guess until a few hundred PR runs show the real
  drift. Tune it from data.
- Valgrind support for new aarch64 instructions can lag. Pin the Valgrind version in CI.
- Device benchmarks depend on whatever hardware the maintainer has. The device list is recorded
  with every result.
- Whether CodSpeed macro runners can be enabled for a public repository in our organisation is
  **unverified**.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| criterion | 0.8.2 (2026-02-04) | 2026-11-06 | crates.io API |
| gungraun | 0.20.0 (2026-09-26) | 2026-11-06 | crates.io API |
| tango-bench | 0.9.0 (2026-10-16) | 2026-11-06 | crates.io API |
| divan | 0.1.21 (2025-04-10) | 2026-11-06 | crates.io API |
| codspeed-criterion-compat | 6.0.0 (2026-10-21) | 2026-11-06 | crates.io API |
| dhat | 0.3.3 (2024-02-04) | 2026-11-06 | crates.io API |
| hyperfine | v2.0.0 (2026-10-07) | 2026-11-06 | GitHub releases |
| Google Benchmark | v1.10.0 (2026-10-14) | 2026-11-06 | GitHub releases |
| package-benchmark (Swift) | 1.38.1 (2026-10-27) | 2026-11-06 | GitHub releases |
| CodSpeed action | v5.6.3 (2026-11-03) | 2026-11-06 | GitHub releases |
| Bencher | v0.6.16 (2026-10-31) | 2026-11-06 | GitHub releases |
| github-action-benchmark | v1.22.2 (2026-09-15) | 2026-11-06 | GitHub releases |
| Toxiproxy | v2.13.0 | 2026-11-06 | GitHub releases |
| pyperf | 2.10.0 | 2026-11-06 | PyPI JSON API |
| pytest-benchmark | 5.3.1 | 2026-11-06 | PyPI JSON API |
| mitata | 1.0.34 | 2026-11-06 | npm registry |
| tinybench | 6.2.3 | 2026-11-06 | npm registry |
| Playwright | 1.65.0 | 2026-11-06 | npm registry |
| benchmark_harness (Dart) | 2.4.0 | 2026-11-06 | pub.dev API |
| BenchmarkDotNet | 0.15.8 | 2026-11-06 | NuGet |
| JMH | 1.37 | 2026-11-06 | Maven Central |
| androidx.benchmark | 1.5.0 (1.6.0-alpha01 newest) | 2026-11-06 | Google Maven |
| Go | 1.27.3 | 2026-11-06 | go.dev/dl |

## 9. Sources

- [CodSpeed pricing](https://codspeed.io/pricing), accessed 2026-11-06.
- [CodSpeed: more free macro runner minutes (2025-09-08)](https://codspeed.io/changelog/2025-09-08-more-free-macro-runners-minutes), accessed 2026-11-06.
- [CodSpeed macro runners docs](https://codspeed.io/docs/features/macro-runners), accessed 2026-11-06.
- [CodSpeed Walltime instrument](https://codspeed.io/docs/instruments/walltime), accessed 2026-11-06.
- [CodSpeed: unrelated benchmark regression](https://codspeed.io/blog/unrelated-benchmark-regression), accessed 2026-11-06.
- [Deterministic instruction counts with Callgrind (hotpath.rs)](https://hotpath.rs/callgrind), accessed 2026-11-06.
- [gungraun docs](https://docs.rs/gungraun/latest/gungraun/), accessed 2026-11-06.
- [Rustls ci-bench](https://git.brokenco.de/rust/rustls/src/branch/alpha-5/ci-bench), accessed 2026-11-06.
- [Bencher: track benchmarks in CI](https://bencher.dev/docs/how-to/track-benchmarks), accessed 2026-11-06.
- [Bencher repository](https://github.com/bencherdev/bencher), accessed 2026-11-06.
