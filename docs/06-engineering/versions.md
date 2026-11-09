---
title: "Versions register"
status: living
updated: 2026-11-09
---

# Versions register

Every pinned tool, toolchain and major dependency, with when it was checked. Check live
(registries and release notes) and record here; review monthly.

| Thing | Version | Checked | Source | Recheck |
|---|---|---|---|---|
| Rust toolchain (stable) | 1.99.1 | 2026-11-06 | `rustc --version` | 2026-12-06 |
| Rust edition | 2024 | 2026-11-06 | Rust reference | — |
| Node.js (site, Node binding) | 26.13.0 | 2026-11-06 | `node --version` | 2026-12-06 |
| pnpm | 12.16.1 | 2026-11-06 | `pnpm --version` | 2026-12-06 |
| Contributor Covenant | 3.0 | 2026-11-06 | EthicalSource repository | 2027-05-06 |
| DCO | 1.1 | 2026-11-06 | developercertificate.org | — |
| napi / napi-derive / napi-build (napi-rs) | 3.16.1 / 3.7.0 / 2.6.0 | 2026-11-07 | crates.io | 2026-12-07 |
| @napi-rs/cli | 3.10.12 | 2026-11-06 | npm | 2026-12-06 |
| wasm-bindgen | 0.2.130 | 2026-11-06 | crates.io | 2026-12-06 |
| binaryen (wasm-opt) | version_133 | 2026-11-06 | GitHub releases | 2026-12-06 |
| PyO3 | 0.30.1 | 2026-11-07 | crates.io | 2026-12-07 |
| maturin | 1.16 | 2026-11-06 | PyPI | 2026-12-06 |
| cibuildwheel | 4.3 | 2026-11-06 | PyPI | 2026-12-06 |
| jni (crate) | 0.22.4 | 2026-11-07 | crates.io | 2026-12-07 |
| cargo-ndk | 4.1 | 2026-11-06 | crates.io | 2026-12-06 |
| Android NDK | r30 | 2026-11-06 | developer.android.com | 2026-12-06 |
| cbindgen (header cross-check) | 0.29.4 | 2026-11-07 | crates.io | 2026-12-07 |
| syn / proc-macro2 (IDL extractor) | 3.0.9 / 1.0.107 | 2026-11-07 | crates.io | 2026-12-07 |
| minijinja (binding templates) | 3.0.5 | 2026-11-07 | crates.io | 2026-12-07 |
| zerocopy (pack views, `derive`) | 0.8.63 | 2026-11-07 | crates.io | 2026-12-07 |
| blake3 (section and head hashes, no default features) | 1.8.7 | 2026-11-07 | crates.io | 2026-12-07 |
| ed25519-dalek (pack signatures, no default features) | 3.0.0 | 2026-11-07 | crates.io | 2026-12-07 |
| ruzstd (BLOBS decoding, feature `zstd`, no default features) | 0.9.1 | 2026-11-07 | crates.io | 2026-12-07 |
| libm (PLACE_INDEX haversine, feature `spatial`, no default features) | 0.2.16 | 2026-11-07 | crates.io | 2026-12-07 |
| zstd (C zstd 1.5.7 via `zstd-sys` 2.1.2; BLOBS compression in tests, benches and the forge) | 0.14.3 | 2026-11-07 | crates.io | 2026-12-07 |
| ruzstd (transport frame decoding in `teigeodata-sync`, feature `zstd`) | 0.9.1 | 2026-11-09 | crates.io | 2026-12-09 |
| zeekstd (seekable-format oracle, dev-dependency of `teigeodata-sync` only; over `zstd-safe` 8.2.0) | 0.6.3 | 2026-11-09 | crates.io | 2026-12-09 |
| zstd seekable format (`contrib/seekable_format`, facebook/zstd) | 0.1.0 | 2026-11-09 | the spec on GitHub | 2027-02-09 |
| criterion (wall-clock benchmarks, `cargo_bench_support` only) | 0.8.2 | 2026-11-07 | crates.io | 2026-12-07 |
| bitpacking (B1 rival, dev-dependency of `teigeodata-bench` only) | 0.9.3 | 2026-11-07 | crates.io | 2026-12-07 |
| geo-index (B8 rival, flatbush with f64 boxes; dev-dependency of `teigeodata-bench` only) | 0.4.0 | 2026-11-07 | crates.io | 2026-12-07 |
| rstar (B8 rival, R*-tree; dev-dependency of `teigeodata-bench` only) | 0.14.0 | 2026-11-07 | crates.io | 2026-12-07 |
| tzf-rs (B9 rival, `default-features = false`; dev-dependency of `teigeodata-bench` only) | 2.3.0 | 2026-11-07 | crates.io | 2026-12-07 |
| tzf-dist `full.tzb` (B9 rival data, release asset `v0.0.2026-f`, SHA-256 `75513a16…35d8a3`) | 2026f | 2026-11-07 | GitHub release of ringsaturn/tzf-dist | 2026-12-07 |
| serde, serde_json (B9 example reads the tbb GeoJSON; dev-dependencies of `teigeodata-bench` only) | 1.0.229, 1.0.154 | 2026-11-07 | crates.io | 2026-12-07 |
| Dart `ffi` package | 2.2.0 | 2026-11-07 | pub.dev | 2026-12-07 |
| Java (JVM binding, CI) | 25 (Temurin) | 2026-11-07 | `java --version` | 2026-12-07 |
| Swift (SwiftPM, local) | 6.4.2 | 2026-11-07 | `swift --version` | 2026-12-07 |
| Python (local) | 3.15.2 | 2026-11-07 | `python3 --version` | 2026-12-07 |
| taiki-e/install-action / setup-java / setup-dart | v2.90.2 / v6.2.0 / v1.8.1 | 2026-11-07 | GitHub | 2026-12-07 |
| Flutter / Dart | 3.47.9 / 3.13.7 | 2026-11-06 | flutter.dev | 2026-12-06 |
| native_toolchain_rust | 1.0.7 | 2026-11-06 | pub.dev | 2026-12-06 |
| rmcp (MCP Rust SDK) | 3.9.0 | 2026-11-06 | crates.io | 2026-12-06 |
| MCP protocol | 2026-07-28 (fallback 2025-11-25) | 2026-11-06 | modelcontextprotocol.io | 2027-01-06 |
| purego (Go) | 0.11 | 2026-11-06 | pkg.go.dev | 2026-12-06 |
| GitHub actions/checkout | v7.0.1 | 2026-11-06 | GitHub | 2026-12-06 |
| Swatinem/rust-cache | v2.9.3 | 2026-11-06 | GitHub | 2026-12-06 |
| GitHub actions/cache | v6.1.0 | 2026-11-07 | GitHub | 2026-12-07 |
| GitHub actions/upload-artifact | v7.0.2 | 2026-11-07 | GitHub | 2026-12-07 |
| cargo-fuzz | 0.13.2 | 2026-11-07 | crates.io | 2026-12-07 |
| cargo-nextest (nightly Miri) | 0.9.151 | 2026-11-09 | GitHub releases | 2026-12-09 |
| libfuzzer-sys | 0.4.13 | 2026-11-07 | crates.io | 2026-12-07 |
| Rust nightly (cargo-fuzz, Miri; unpinned in CI, latest each night) | 1.101.0-nightly 2026-10-20 locally | 2026-11-07 | `rustc +nightly --version` | 2026-12-07 |
| cargo-deny (action / local) | v2.1.1 / 0.20.2 | 2026-11-06 | GitHub / Homebrew | 2026-12-06 |
| typos | v1.52.0 | 2026-11-06 | GitHub | 2026-12-06 |
| criterion | 0.8.2 | 2026-11-06 | crates.io | 2026-12-06 |
| gungraun and gungraun-runner (the `counts` CI gate; the versions must match) | 0.20.0 | 2026-11-07 | crates.io | 2026-12-07 |
| tango-bench | 0.9.0 | 2026-11-06 | crates.io | 2026-12-06 |
| hyperfine | 2.0.0 | 2026-11-06 | GitHub releases | 2026-12-06 |
| Google Benchmark | 1.10.0 | 2026-11-06 | GitHub releases | 2026-12-06 |
| pyperf | 2.10.0 | 2026-11-06 | PyPI | 2026-12-06 |
| mitata | 1.0.34 | 2026-11-06 | npm | 2026-12-06 |
| Playwright | 1.65.0 | 2026-11-06 | npm | 2026-12-06 |
| JMH | 1.37 | 2026-11-06 | Maven Central | 2026-12-06 |
| androidx.benchmark | 1.5.0 | 2026-11-06 | Google Maven | 2026-12-06 |
| package-benchmark (Swift) | 1.38.1 | 2026-11-06 | GitHub releases | 2026-12-06 |
| BenchmarkDotNet | 0.15.8 | 2026-11-06 | NuGet | 2026-12-06 |
| benchmark_harness (Dart) | 2.4.0 | 2026-11-06 | pub.dev | 2026-12-06 |
| Go | 1.27.3 | 2026-11-06 | go.dev | 2026-12-06 |
| osmpbf | 0.3.8 | 2026-11-06 | crates.io | 2026-12-06 |
| parquet / arrow | 60.1 | 2026-11-06 | crates.io | 2026-12-06 |
| geoarrow / geoparquet | 0.10 | 2026-11-06 | crates.io | 2026-12-06 |
| i_overlay | 9.1 | 2026-11-06 | crates.io | 2026-12-06 |
| geo | 0.33 | 2026-11-06 | crates.io | 2026-12-06 |
| tiff | 0.11.3 | 2026-11-06 | crates.io | 2026-12-06 |
| duckdb (QA only) | 1.20000.1 (DuckDB 2.0) | 2026-11-06 | crates.io | 2026-12-06 |
| ed25519-dalek | 3.0 | 2026-11-06 | crates.io | 2026-12-06 |
| blake3 | 1.8 | 2026-11-06 | crates.io | 2026-12-06 |
| ureq | 3.5 | 2026-11-06 | crates.io | 2026-12-06 |
| tough (test oracle) | 0.24 | 2026-11-06 | crates.io | 2026-12-06 |
| serde-saphyr (xtask YAML) | 2.1.1 | 2026-11-06 | crates.io (serde_yaml is deprecated) | 2026-12-06 |
| fst (search spike; own automaton, not its Levenshtein DFA) | 0.4.7 | 2026-11-06 | crates.io (last release 2021-06-06) | 2026-12-06 |
| calamine (COD-AB workbooks, spikes) | 0.38.0 | 2026-11-06 | crates.io | 2026-12-06 |
| unicode-script | 0.5.9 | 2026-11-06 | crates.io | 2026-12-06 |
| unicode-normalization | 0.1.26 | 2026-11-06 | crates.io | 2026-12-06 |
| sha2 (forge fetch) | 0.11.1 | 2026-11-06 | crates.io | 2026-12-06 |
| bytes (forge: in-memory Parquet) | 1.12.1 | 2026-11-07 | crates.io | 2026-12-07 |
| SlideRule server (ATL08 reference, S6) | v5.8.0 | 2026-11-07 | `/source/version` | 2026-12-07 |
| GeographicLib geoid grid egm2008-5 | 2009-08-29 | 2026-11-07 | SourceForge `geoids-distrib` | 2027-11-07 |
| toml (lock files) | 1.2.1 | 2026-11-06 | crates.io | 2026-12-06 |
| zip (forge readers; releases almost daily) | 9.4.1 | 2026-11-06 | crates.io | 2026-12-06 |
| arrow-array / arrow-schema (forge) | 60.1.0 | 2026-11-06 | crates.io | 2026-12-06 |
| QLever (Wikidata endpoint, moved to qlever.dev) | service | 2026-11-06 | qlever.dev | 2026-12-06 |
| TUF specification | 1.0.37 | 2026-11-06 | theupdateframework.github.io | 2027-02-06 |
| memmap2 (engine, mapped packs) | 0.9.12 | 2026-11-09 | crates.io | 2026-12-09 |
| arc-swap (engine `Shared`, D-141) | 1.9.4 | 2026-11-09 | crates.io | 2026-12-09 |

Libraries chosen in Phase 0 research are added here when adopted.
