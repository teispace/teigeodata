---
title: "Phase 4 benchmarks: the engine"
status: research
updated: 2026-11-09
---

# Phase 4 benchmarks: the engine

Measured with the engine's own example. IDs are from the
[benchmark catalogue](../../06-engineering/benchmarks.md); the design is the
[engine page](../../04-architecture/engine.md) and the
[engine and catalog note](../notes/engine-and-catalog.md).

```
cargo run --release -p teigeodata-engine --example measure -- <pack.tgd | synthetic:N>
```

**Machine:** M2 Pro, 16 GB, macOS, release profile, single thread, 2026-11-09, load average
7 to 10 (`uptime` at 10:05; shared with other builds). Every warm figure is the minimum of 7
rounds of 2,000 probes (200 for search), with the configurations alternating inside a round.
The numbers show the shape, not a budget.

**Input:** `synthetic:200000`, the generator of the engine tests: 200,000 places in a
4° × 8° box, a country and 8 districts as ADMIN_REGION, 2 timezone zones, a 5.28 MB pack. The
Nepal main pack (D-117 build, 11.7 MB) is measured in [its own section](#nepal-main-pack).

## L1, L4: open and first query

| Measure | Time |
|---|---:|
| Open, mapped (head checks, block-hash proofs), first | 412 µs |
| Open, mapped, warm | 89.9 µs |
| First `get` (verifies IDMAP, REDIRECTS, PLACES) | 1.88 ms |
| First `reverse` (PLACE_INDEX) | 169 µs |
| First `admin_at` (ADMIN_REGION, ARCS, POLYGONS) | 75 µs |
| First `tz_at` (REGION) | 28 µs |
| First `search` (SEARCH) | 602 µs |

## L5: warm queries, engine against direct section calls

The 3-layer engine holds the same places split by a hash of the ID into three packs (every
admin and zone place in each, as the metamorphic gate splits them).

| Query | direct (ns) | engine, 1 layer (ns) | engine, 3 layers (ns) |
|---|---:|---:|---:|
| `get` | 176 | 176 | 394 |
| `reverse`, k = 10 | 4,860 | 5,742 | 20,043 |
| `admin_at` (direct: region IDs only) | 60 | 148 | 144 |
| `tz_at` (direct: zone row only) | 25 | 64 | 63 |
| `search`, k = 10 | n/a | 33,830 | 68,681 |

`admin_at` and `tz_at` through the engine also read each unit's row and check that no layer
above claims it, which the direct column does not do.

Two changes cut the multi-layer cost before these numbers:
- `reverse` checks visibility only for places that can still enter the answer, and a layer is
  done as soon as its next distance cannot beat the k-th; before: 10,454 ns (1 layer) and
  49,178 ns (3 layers);
- `search` reads IDs only for groups of equal score that can still enter its top k; before:
  47,562 ns and 242,745 ns.

The 3-layer `reverse` is about 3.5× one layer because each layer fetches its own k + 1; the
latitude bound prunes layers only when they are far apart, which a split by ID never is.
Packs split by area (the real case: countries and provinces) prune better.

## Gates

| Gate | Result |
|---|---|
| Metamorphic, synthetic (600, 1,500, 900 places in 2, 3, 5 parts, both orders, an overlapping copy) | pass |
| Metamorphic, Nepal main pack in 2 and 3 parts (`TGD_PACK`, release) | pass, 46.7 s |
| Differential: delta layer against the compacted pack | pass |
| M1: warm allocations (`tests/alloc.rs`) | 0 for `get`, `reverse`, `admin_at`, `tz_at`, `within_bbox`; 1 for `search` (the folded query) |

Planted bugs, each run and reverted (D-114):

| Plant | Caught by |
|---|---|
| Visibility check always false | 3 metamorphic tests |
| Prefix matches not cut to the best 16 | split and differential tests |
| `reverse` never asks a layer for more | differential test (the split test alone missed it, so the differential test was added) |
| Lazy section check always passes | `a_corrupt_section_fails_only_the_queries_that_read_it` |
| Block check skipped | `a_large_section_is_checked_a_block_at_a_time` |

## Nepal main pack

`data/build/pack/np/np-places-main.tgd`, built 2026-11-09 with corrections (D-117), 101,550
places. Same machine and method, load average 5 (`uptime` at 10:18). The metamorphic gate on
this pack (`TGD_PACK`, release) passed in 3.9 s.

| Measure | Time |
|---|---:|
| Open, mapped (head checks, block-hash proofs), first | 296 µs |
| Open, mapped, warm | 163 µs |
| First `get` (IDMAP, REDIRECTS, PLACES) | 468 µs |
| First `reverse` (PLACE_INDEX) | 45 µs |
| First `admin_at` (ADMIN_REGION, ARCS, POLYGONS) | 2.21 ms |
| First `tz_at` (REGION) | 1.19 ms |
| First `search` (SEARCH) | 1.61 ms |

| Query | direct (ns) | engine, 1 layer (ns) | engine, 3 layers (ns) |
|---|---:|---:|---:|
| get | 121 | 130 | 298 |
| reverse, k = 10 | 6,153 | 6,578 | 23,329 |
| admin_at (direct: region IDs only) | 151 | 368 | 428 |
| tz_at (direct: zone row only) | 44 | 84 | 105 |
| search, k = 10 | n/a | 72,598 | 163,948 |

The first `admin_at` and `tz_at` pay for verifying ARCS (2.66 MB) and REGION (2.0 MB): the
size ideas for those two sections ([ideas](../../01-product/ideas.md)) also shorten the first
query. One layer costs at most 2.2× a direct call (admin_at, which also builds the hierarchy);
three layers cost 2.3× to 3.8× one layer for get, reverse and search.

### Search after D-119

Same pack, same machine, 2026-11-09 (load about 5). `measure`: search k = 10 through the engine
with 1 layer 11.1 µs (was 72.6 µs), 3 layers 24.8 µs (was 163.9 µs). The golden set scored on
the engine (`S3_ENGINE=product`): recall@1 0.652, recall@5 0.766, pass 0.725, p50 6.7 µs, p99
78 µs; the spike's search on the same pack: 0.801, 0.932, 0.887.

### Search after D-120

Same pack built with the query rules, 2026-11-09, load 16 to 58 (another project's builds).
The golden set, run back to back:

| Search | recall@1 | recall@5 | pass | MRR@10 | p50 | p99 |
|---|---:|---:|---:|---:|---:|---:|
| Product (`S3_ENGINE=product`) | 0.797 | 0.932 | 0.885 | 0.861 | 6.7 µs | 77 µs |
| Spike | 0.801 | 0.932 | 0.887 | 0.863 | 7.2 µs | 77 µs |
| Product, rules decoded per query (first build) | 0.797 | 0.932 | 0.885 | 0.861 | 11.7 µs | 91 µs |
| Product before D-120 (D-119) | 0.652 | 0.766 | 0.725 | 0.707 | 6.6 µs | 80 µs |

Per class against the spike: abbreviation 0.951 and 0.951, mixed script 0.857 and 0.886 (the
product reads only accepted lexicon terms), with context 0.983 and 1.000. Turning the phonetic
tier off saves 0.3 µs and costs 0.012 recall@1. `measure`: search k = 10 through the engine,
1 layer 12.3 µs, 3 layers 28.1 µs (its own query mix, load 17).

### Radius queries (D-140)

Same pack rebuilt 2026-11-09 (D-139), same machine, load about 8 to 15. `measure` against the
first table:

| Query | engine, 1 layer (ns) | engine, 3 layers (ns) |
|---|---:|---:|
| reverse, k = 10 | 6,190 (was 6,578) | 15,990 (was 23,329) |
| within_radius 1 km, k = 50 | 2,791 | 7,743 |
| within_radius 10 km, k = 50 | 12,022 | 40,711 |

`reverse` across layers is faster because each layer's index search now starts from the k-th
distance of the layers before (`nearest_within`) instead of ranking its own k first: 31% on
3 layers. A radius query opens no index box beyond its radius.

### Snapshot swap A/B (D-141)

`cargo run --release -p teigeodata-engine --example snapshot-ab`, 2026-11-09, load about 11.
3 layers of 50,000 synthetic places; `get` with no holder 363 ns. Readers: 8 threads, each
`get` through the holder; a writer clones the current engine and publishes it every 100 µs
for 1.5 s. Two rounds:

| Holder | 1 thread (ns/get) | 8 readers + writer (ns/get per reader) | Worst publish |
|---|---:|---:|---:|
| `arc-swap` `load()` | 367, 372 | 510, 555 | 0.45, 0.32 ms |
| `RwLock`, held for the query | 410, 373 | 1,864, 1,665 | 9.9, 26.4 ms |
| `RwLock`, `Arc` cloned | 365, 378 | 3,110, 2,913 | 13.7, 10.3 ms |
| `Mutex`, `Arc` cloned | 369, 432 | 4,990, 5,283 | 0.25, 1.0 ms |

On one thread all four cost the same as a bare query. Under contention `arc-swap` costs 1.4×
a bare query; the others cost 4.6× to 14× because every reader writes the lock's (or the
`Arc`'s) shared counter. A held `RwLock` also starves the writer.
