---
title: "Benchmarks"
status: living
updated: 2026-11-06
---

# Benchmarks

The catalogue of everything we measure for speed, size and memory: which suites exist, which
cases each one runs, on which platforms and bindings, and which gate uses it. The research behind
the method is in [Benchmarking](../02-research/notes/benchmarking.md). Accuracy, topology and
search quality are data gates in [the quality bar](quality-bar.md), not benchmarks. Search
latency is a benchmark here.

Results are generated into `BENCHMARKS.md` and `SIZES.md` and are never hand-written. A number
quoted anywhere else carries its suite ID, its date and its machine.

## Method

- **One workload, many harnesses.** Workloads are defined once in `teigeodata-bench`. Query
  files live in `bench/queries/` (fixed seeds, checked in, with SHA-256 hashes). Every harness and
  every binding runs the same files.
- **Harnesses:**
  - Rust: `gungraun` (instruction counts, cache, heap; the PR gate), `criterion` (wall-clock,
    nightly) and `tango-bench` (paired A/B for research).
  - CLI: `hyperfine`.
  - Bindings: each language's standard harness (table below).
- **Statistics:** at least 30 samples after warm-up. Report p50, p90, p99 and p99.9, the mean
  with a 95 % confidence interval, throughput, and for allocations the count and bytes.
  Comparisons use Welch's t-test; "faster" claims need p < 0.01 and an effect above noise.
- **Hygiene:**
  - `black_box` on inputs and outputs.
  - Release profile as shipped (`dist` for size suites).
  - Record the machine (CPU, cores, RAM, OS, governor, load average, commit, rustc).
  - Local runs refuse to start above a load average of 2 unless forced, and say so in the result.
  - Cold-cache cases drop the page cache (Linux `drop_caches`, macOS `purge`) or use a fresh
    copy of the file.
- **Baselines** are stored per runner class (`gh-ubuntu-x64`, `gh-ubuntu-arm64`, `gh-macos-arm64`,
  `gh-windows-x64`, `local-<name>`, `device-<model>`). A number is never compared across
  classes.
- **Every new benchmark gate is proven red** with a deliberate regression (for example a sleep
  or an extra allocation) before it is trusted.

## Cases (the axes every suite picks from)

| Axis | Values |
|---|---|
| Pack | Nepal (small), India (large), South Asia, world timezone pack, world lite; full and feature-subset builds |
| Query distribution | uniform over the world; uniform over the pack's land; population-weighted; real places (GeoNames, OSM); golden-set queries; adversarial (1 unit from an edge, on vertices, on the antimeridian and poles, ocean, outside the pack) |
| Cache state | hot (repeated), warm (pack in page cache, random order), cold (page cache dropped, first query after open) |
| Batch | single call; batches of 10, 1,000 and 100,000 (the bindings' batch APIs) |
| Threads | 1, 2, 4, 8, all cores (shared reader) |
| Script (search) | Latin, Devanagari, mixed, misspelt (distance 1 and 2), romanisation variants (Chowk/Chok), prefixes of 1–5 characters |
| Overlay | compiled-in pack only; plus 1, 4 and 16 downloaded packs merged live |

## Suites

### Q: queries (Rust core and every binding)

| ID | Operation | Metric | Rival or reference |
|---|---|---|---|
| Q1 | Reverse geocode: nearest place, k = 1/10/100, with filters | ns/query, instructions | `rstar`, `kdbush`, reverse_geocoder |
| Q2 | Point to admin chain (country down to ward) | ns/query | ray casting with `geo` |
| Q3 | Point to IANA timezone; plus the UTC offset at an instant | ns/query | `tzf-rs` (lite and full data) |
| Q4 | Point to elevation (bilinear, and the uncertainty field) | ns/query | direct GeoTIFF read with GDAL |
| Q5 | Point to postal code (current and former) | ns/query | none |
| Q6 | Search: exact, prefix completion top-k, fuzzy d = 1/2, phonetic, cross-script | µs/query, p99 | `fst` alone, tantivy, Photon (local) |
| Q7 | Get by ID, hit and miss; batch of IDs | ns/lookup | sorted array with binary search |
| Q8 | Bbox and radius queries returning N places | µs/query, rows/s | `rstar`, flatbush |
| Q9 | Polygon fetch and decode per level of detail; to GeoJSON and to WKB | µs/polygon, MB/s | GeoJSON parse with `serde_json` |
| Q10 | Field projection: one field against all fields per row (tree-shaken columns) | ns/row | none |

### L: open, verify and load

| ID | Operation | Metric |
|---|---|---|
| L1 | Open a pack (mmap and header validation) at 1, 10, 100 MB and 1 GB | µs |
| L2 | Integrity: lazy against eager BLAKE3, 16 KiB against 64 KiB blocks (format B12) | ms, CPU % |
| L3 | Ed25519 signature and catalog metadata verification | µs |
| L4 | First query after open, cold and warm | µs |
| L5 | Live merge of the compiled-in pack with N downloaded packs, then queries across them | µs open, ns/query overhead |
| L6 | Open from Android assets (`noCompress`), iOS bundle and WASM `ArrayBuffer`/OPFS | µs |

### S: sync (download, update, repair)

These run against the local network shaper: latency 0/50/300 ms; bandwidth 1/10/100 Mbit/s; 1 %
packet loss; a host that ignores `If-Range`; a mirror that returns bad bytes.

| ID | Operation | Metric |
|---|---|---|
| S1 | Catalog fetch and verification (TUF-style roles) | ms, bytes |
| S2 | Full pack download with resume after interruption at a random point | s, bytes over the wire, correctness |
| S3 | Update by changed sections (range requests) against a full download | bytes saved, s |
| S4 | Atomic install with a crash injected at each step (journal recovery) | recovery ms, correctness |
| S5 | Repair of a corrupted section | bytes, s |

### M: memory

| ID | Measure | Gate |
|---|---|---|
| M1 | Allocations per query for Q1–Q8 (counting allocator, and `gungraun` DHAT) | zero where designed; PR |
| M2 | RSS after open and after 10⁶ queries (mapped pages against heap) | nightly |
| M3 | Peak heap and leaks in every binding (Valgrind, ASan, language tools) | nightly |
| M4 | Peak RSS of a data build per country (forge) | data build |

### Z: sizes (written to `SIZES.md`)

| ID | Measure |
|---|---|
| Z1 | Pack bytes per country, per component (points, names, polygons per LOD, tz, DEM, search) and per field |
| Z2 | Library bytes per target (Linux, macOS, Windows, Android ABIs, iOS, WASM) and per feature set: minimal (lookup only), tz only, search only, full. `cargo xtask sizes` (`tools/size-probe`, a std baseline subtracted) |
| Z3 | WASM after `wasm-opt`, raw and Brotli |
| Z4 | Binding package size (wheel, npm, AAR, XCFramework, pub, NuGet, Go module) |
| Z5 | Tree-shaking proof: an app using only timezone lookup contains no search or polygon code (symbol diff) |

### X: bindings (FFI cost)

Each binding runs Q1, Q3, Q6, Q7 and L1 from the same query files, both single calls and
batches, and reports **binding time minus Rust time** as the marshalling cost.

| Language | Harness | Platforms |
|---|---|---|
| C, C++ | Google Benchmark | Linux, macOS, Windows |
| Python | `pyperf` (and `pytest-benchmark` for suites) | Linux, macOS, Windows; CPython 3.10+ and free-threaded |
| Node | `mitata` | Linux, macOS, Windows |
| WASM | `mitata` in Chrome, Firefox and WebKit via Playwright; plus Node and Deno | browsers |
| Java, Kotlin (JVM) | JMH | Linux, macOS, Windows |
| Android | `androidx.benchmark` microbenchmarks | real devices (low, mid, high tier) |
| Swift | `package-benchmark`; XCTest `measure` on devices | macOS; iPhone |
| .NET | BenchmarkDotNet | Linux, macOS, Windows |
| Go | `testing.B` with `b.Loop` | Linux, macOS, Windows |
| Dart | `benchmark_harness` | Linux, macOS, Windows |
| Flutter | `integration_test` with timeline summaries | Android and iOS devices |
| MCP server | request/response latency per tool over stdio and HTTP | Linux |

### C: competitive (published with each release)

Run on one documented machine with the same query files. Rivals: GeoNames-backed libraries,
`tzf-rs`, Nominatim and Photon (local Docker), Pelias, and Overture through DuckDB. For each
rival we report latency, size on disk, RAM, setup time and accuracy on the same golden set.
Results include the rival's version and configuration, and we invite their maintainers to check
them ([Competitive analysis](../02-research/notes/competitive-analysis.md)).

### D: data builds (forge)

| ID | Measure |
|---|---|
| D1 | Build time and peak RSS per country and per stage |
| D2 | Determinism: two builds give byte-identical packs |
| D3 | Throughput of conflation and the geometric median per 100 k records |
| D4 | Incremental rebuild after one source changes (cache hit rate) |

The forge note's own benchmarks (reader speed, polygon assembly against osmium, DEM decode
against GDAL, conflation throughput) are listed there as B1–B4 and are referred to here as
**FB1–FB4** to keep them apart from the format benchmarks.

### Research A/B (decide a design, then retire)

The format note's [A/B list](../02-research/notes/format-compression-indexing.md) **B1–B14**
(integer blocks, row order, names, zstd decoders, ID maps, rank/select, top-k, place index,
region index, polygons, DEM codecs, integrity, zero-copy checks, codec selection) runs with
`tango-bench` on native x86-64 and aarch64, and in WASM. Each one ends with a recorded decision
and a page in `docs/02-research/benches/`. The winning layout then gets a permanent Q, L or Z
benchmark.

## Platforms and when each runs

| Runner class | What runs | When |
|---|---|---|
| `gh-ubuntu-x64`, `gh-ubuntu-arm64` | `gungraun` Q, L, M1 (gate); Z2–Z5 | every PR touching hot paths or the build |
| same | `criterion` Q, L, S; X for C, Python, Node, JVM, .NET, Go, Dart | nightly |
| `gh-macos-arm64`, `gh-windows-x64` | `criterion` Q and L; X for their platforms (coarse numbers) | nightly |
| browsers (Playwright on Linux) | WASM X, L6 | nightly |
| `local-<machine>` | everything, A/B research, C | on demand, before releases |
| `device-<model>` | Android and iOS X, L6, Flutter | each release, by hand, on the device list |

## Gates

| Gate | Rule | Where |
|---|---|---|
| Instructions | Q and L instruction counts within +1 % of the baseline (threshold tuned from data) | PR |
| Allocations | M1 counts unchanged; zero where designed | PR |
| Sizes | Z1–Z4 within budget; any growth over 1 % explained in the PR body | PR |
| Wall-clock | nightly `criterion` regressions with p < 0.01 open an issue | nightly |
| Bindings | marshalling cost per call within its budget (set after spike S4) | nightly |
| Release | full matrix, devices, and the competitive report attached to the release | release |

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-06 | Catalogue created | Maintainer asked for proper benchmarks across features, cases and bindings; [research](../02-research/notes/benchmarking.md) |
