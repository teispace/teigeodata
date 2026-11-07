---
title: "Phase 1 benchmarks: core types and the container"
status: research
updated: 2026-11-07
---

# Phase 1 benchmarks: core types and the container

Measured with `crates/teigeodata-bench` (criterion 0.8.2):

```
cargo bench -p teigeodata-bench --bench core --bench format -- --warm-up-time 1 --measurement-time 3
```

The workloads come from fixed seeds (`teigeodata_bench::{world_points, pack_of_size}`), so every
harness and, later, every binding runs the same inputs. IDs are from the
[benchmark catalogue](../../06-engineering/benchmarks.md).

**Machine:** M2 Pro, 16 GB, macOS, rustc 1.99.1, release profile, single thread, 2026-11-07.
The machine was shared: load averages were 5 to 14 during the runs. That is above the catalogue's
local limit of 2, so these are indicative numbers. The PR gate will use gungraun's instruction
counts in CI, which do not depend on load.

## Core types

| Case | Time | Notes |
|---|---:|---|
| Hilbert index, prefix-scan version | 9.4 ns per point (106 M/s) | 100,000 world points, 1e-7° grid |
| Hilbert index, classic loop (the definition) | 44.9 ns per point (22 M/s) | the same points |
| `PlaceId` to text, 1 / 3 / 10 base32 digits | 9.1 / 11.2 / 17.9 ns | no allocation |
| `PlaceId` from text, 1 / 3 / 10 digits | 5.9 / 8.7 / 21.2 ns | |

The prefix-scan Hilbert index is **4.75× faster** than the loop it is proven equal to.

## Container (L1 to L3)

Packs of 1, 10 and 100 MB: three sections of pseudo-random bytes, a manifest and a signature.

| ID | Case | 1 MB | 10 MB | 100 MB |
|---|---|---:|---:|---:|
| L1 | `Pack::open`: every structural check and the head digest | 0.71 µs | 0.70 µs | 0.71 µs |
| L2 | `verify_sections`: BLAKE3 of every section, eager | 0.55 ms (1.76 GiB/s) | 5.5 ms (1.76 GiB/s) | 55.5 ms (1.76 GiB/s) |
| L3 | `verify_signature` (Ed25519 `verify_strict`) | 34.4 µs | | |

**Findings:**
- **Opening costs the same at every size,** because `open` reads only the head (at most 16 KiB).
  A 100 MB pack is usable after 0.7 µs. Hashing is what scales with size: eager hashing of
  100 MB takes 55 ms on one core. B12 decides when sections are hashed (eager, on first use, or
  per block), and that decides the default.
- **BLAKE3 already uses NEON:** enabling the `neon` feature changed nothing (p = 0.90). The
  runtime keeps `default-features = false`.
- **Ed25519 precomputed tables** (the `fast` feature) verify in 31.9 µs, against 34.4 µs without
  them. That saves 2.5 µs once per pack open, at the cost of larger tables in the library. The
  runtime stays without `fast`; Z2 measures the size. The forge may enable it for signing.
- **1 GB packs were not run:** a pack of that size is built in memory here. The memory-mapped
  L1 case runs when the engine maps files (Phase 4).
