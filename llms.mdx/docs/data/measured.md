# Measured (/docs/data/measured)

<Callout title="Generated from the gates">
This page is written by `cargo xtask data-pages` from the records in
[`site/data/measured`](https://github.com/teispace/teigeodata/tree/main/site/data/measured), each
written by the gate that measured it (`--record`). Every figure says when, where and at which
commit it was measured: compare a timing only with timings from the same machine, and read one
from a busy machine (a high load average) as a ceiling, not a typical value.
</Callout>

The bytes of each component of the released pack are on the [coverage page](/docs/data/coverage);
how good the data is, and how it is gated, is on [accuracy](/docs/concepts/accuracy).

## WebAssembly packages [#webassembly-packages]

Each npm package's module after `wasm-opt -Oz`, raw and gzipped at level 9. `cargo xtask package-wasm` fails a build whose gzipped module is over its budget.

Measured 2026-11-15 on Darwin arm64, 10 cores (load average 10.36), commit [`25ad7e5`](https://github.com/teispace/teigeodata/commit/25ad7e5), by `cargo xtask package-wasm --record`.

| Package | Holds | Module | gzip -9 | Budget | Headroom |
|---|---|--:|--:|--:|--:|
| `teigeodata-wasm` | every component | 605118 B | 253910 B | 262144 B | 3.1% |
| `teigeodata-wasm-lookup` | every query at a point or by ID | 380167 B | 145724 B | 153600 B | 5.1% |

## Library bytes per feature set [#library-bytes-per-feature-set]

What each feature set of the runtime crates adds to a linked library (`tools/size-probe`, a baseline that any host library links subtracted), natively in the release profile and for wasm32 in the size profile.

Measured 2026-11-15 on Darwin arm64, 10 cores (load average 7.04), commit [`25ad7e5`](https://github.com/teispace/teigeodata/commit/25ad7e5), by `cargo xtask sizes --record`.

| Feature set | native B | native gzip | wasm32 B | wasm32 gzip |
|---|---:|---:|---:|---:|
| open: head checks, section lookup | 16.4 KiB | 12.3 KiB | 18.4 KiB | 8.4 KiB |
| open + BLAKE3 of every section | 16.5 KiB | 13.5 KiB | 20.2 KiB | 9.2 KiB |
| open + Ed25519 signature check | 32.7 KiB | 27.8 KiB | 47.6 KiB | 20.0 KiB |
| open + every section view (places, hierarchy, ID map, names, components) | 64.9 KiB | 36.7 KiB | 103.5 KiB | 25.8 KiB |
| open + BLOBS (`ruzstd`) | 65.0 KiB | 43.3 KiB | 124.0 KiB | 34.5 KiB |
| the whole reader: all of the above | 129.7 KiB | 74.1 KiB | 171.8 KiB | 53.6 KiB |
| the writer alone (forge side) | 16.5 KiB | 16.3 KiB | 26.1 KiB | 11.2 KiB |
| the engine, points only: open, `get`, `reverse` | 113.5 KiB | 67.7 KiB | 143.4 KiB | 43.8 KiB |
| the engine, every query | 452.4 KiB | 248.1 KiB | 459.8 KiB | 184.1 KiB |

Baseline (subtracted): native 278.5 KiB, wasm32 16.1 KiB.

## Scaling across threads [#scaling-across-threads]

Throughput of one shared engine on `np-places-lite.tgd`: each thread takes the current snapshot for every query, as a server's handlers do, and runs `get`, `reverse` (10 nearest), `admin_at`, `tz_at` and `search` (10 hits) in turn. The best of three rounds per thread count. The gate fails when four threads reach less than 1.5× one thread, the mark of a lock on the query path; the machine is shared and mixes performance and efficiency cores, so the figures are a floor.

Measured 2026-11-15 on Darwin arm64, 10 cores (load average 6.54), commit [`668ec58`](https://github.com/teispace/teigeodata/commit/668ec58), by `cargo xtask scaling np-places-lite.tgd --record`.

| Threads | Queries/s | Speed-up | Efficiency |
|---:|---:|---:|---:|
| 1 | 207739 | 1.00× | 100% |
| 2 | 416120 | 2.00× | 100% |
| 4 | 811143 | 3.90× | 98% |
| 8 | 1267633 | 6.10× | 76% |

## Server load [#server-load]

`tgd serve` on loopback, each route driven by oha 1.17.0 for 5 s at each point, twice; round two is reported, and the footprint's growth across it is the leak check. `tools/load/budget.toml` sets each point's floor and ceiling.

Measured 2026-11-15 on Darwin arm64, 10 cores (load average 22.79), commit [`3688d14`](https://github.com/teispace/teigeodata/commit/3688d14), by `cargo xtask load <pack> --check tools/load/budget.toml --record`.

**np-places-lite**

| Route | c | req/s | p50 ms | p99 ms | requests | failed |
|---|--:|--:|--:|--:|--:|--:|
| search | 1 | 4793 | 0.134 | 1.843 | 23979 | 0 |
| search | 16 | 64215 | 0.191 | 1.054 | 321181 | 0 |
| search | 64 | 61182 | 0.913 | 3.390 | 306084 | 0 |
| reverse | 16 | 71036 | 0.174 | 1.007 | 355321 | 0 |
| get | 16 | 84291 | 0.154 | 0.800 | 421732 | 0 |
| near | 16 | 63304 | 0.194 | 1.110 | 316628 | 0 |
| hierarchy | 16 | 90183 | 0.157 | 0.530 | 451085 | 0 |
| boundary | 16 | 8875 | 1.551 | 8.369 | 44374 | 0 |
| nominatim | 16 | 53444 | 0.256 | 1.011 | 267380 | 0 |
| mcp search | 16 | 39315 | 0.370 | 1.216 | 196674 | 0 |

footprint: 5616 KB idle, 16384 KB peak, -3072 KB across round two

**world-baseline-lite**

| Route | c | req/s | p50 ms | p99 ms | requests | failed |
|---|--:|--:|--:|--:|--:|--:|
| search | 1 | 4129 | 0.103 | 1.951 | 20649 | 0 |
| search | 16 | 73481 | 0.162 | 0.928 | 367581 | 0 |
| search | 64 | 81055 | 0.676 | 2.875 | 405502 | 0 |
| reverse | 16 | 58112 | 0.187 | 1.325 | 290656 | 0 |
| get | 16 | 78654 | 0.149 | 0.930 | 393402 | 0 |
| near | 16 | 53536 | 0.190 | 1.495 | 267787 | 0 |
| hierarchy | 16 | 82638 | 0.142 | 0.950 | 413413 | 0 |
| boundary | 16 | 31980 | 0.301 | 3.029 | 159944 | 0 |
| nominatim | 16 | 69888 | 0.172 | 1.029 | 349665 | 0 |
| mcp search | 16 | 42750 | 0.292 | 1.498 | 213764 | 0 |

footprint: 5072 KB idle, 10240 KB peak, +0 KB across round two

## Phone-class estimates [#phone-class-estimates]

The phone-class bench (P1, G-phone) on `world-np-lite.tgd`: open, the first query of each kind, search a keystroke at a time and the point queries, each the minimum of several rounds here and multiplied by 6.5 (this machine's core over a low-end phone's), and the heap counted by allocator; the gate fails when an estimate is over its target. A point query is `reverse_in_country` (10 nearest), `admin_at`, `tz_at` and `elevation_at`; the page cache is warm, as on a relaunch.

Measured 2026-11-15 on Darwin arm64, 10 cores (load average 7.63), commit [`41ba69a`](https://github.com/teispace/teigeodata/commit/41ba69a), by `cargo xtask phone world-np-lite.tgd --record`.

| Measure | Here | Phone estimate | Target |
|---|--:|--:|--:|
| open | 133.75µs | 869.38µs | 50ms |
| first queries | 6.06ms | 39.40ms | 150ms |
|   first get | 809.79µs | 5.26ms | – |
|   first reverse | 2.21ms | 14.39ms | – |
|   first admin_at | 458.00ns | 2.98µs | – |
|   first tz_at | 31.62µs | 205.56µs | – |
|   first elevation_at | 286.42µs | 1.86ms | – |
|   first search | 2.57ms | 16.68ms | – |
| keystroke p50 | 9.25µs | 60.12µs | – |
| keystroke p99 | 203.46µs | 1.32ms | 16ms |
| keystroke max | 235.00µs | 1.53ms | 50ms |
| point query p99 | 76.00µs | 494.00µs | 2ms |
| get p99 | 250.00ns | 1.62µs | 500µs |
| derive SUBSTRING | 389.03ms | 2.53s | 3s |
| heap after open and the first queries | 0.05 MiB | | 4 MiB |
| heap peak | 0.05 MiB | | 16 MiB |
| heap peak while deriving SUBSTRING | 55.00 MiB | | 64 MiB |