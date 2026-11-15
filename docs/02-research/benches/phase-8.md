---
title: "Phase 8 benchmarks: profiles and scaling"
status: research
updated: 2026-11-15
---

# Phase 8 benchmarks: profiles and scaling

Phase 8.3's measurements: where each query spends its time, the two changes the profiles led
to, and how one shared engine scales across threads. The method and the verdicts are in the
[profiling note](../notes/profiling.md) (D-338); the gates are on the
[benchmarks page](../../06-engineering/benchmarks.md).

**Machine:** M2 Pro (6 performance, 4 efficiency cores), 16 GB, macOS, release profile with
line tables, 2026-11-15, load average 6 to 11 (shared with other projects' builds). Absolute
times move with the load by up to 2×; the shares within one profile, and A/B pairs run in
turn, are what these pages compare.

## Profiles

```
cargo xtask profile data/build/pack/np/np-places-lite.tgd \
  data/build/baseline/world/world-baseline-lite.tgd --secs 6
```

Instruments' Time Profiler, one suite per process, 6 s each (about 6,000 samples at 1 ms).
"Under our frame" charges each sample to the innermost teigeodata function on its stack, so
library code (`libm`, `memmove`) counts under the function that called it; "self time" is the
leaf frame. The three largest of each, as shares of the suite's samples. Recorded after both
changes below; `get` and `open` are therefore the changed code. The recording of `tz` on
Nepal hung once in `xctrace` and was retried by the watchdog.

### `np-places-lite.tgd`

| Suite | ns per call | Top under our frame (share) | Top self time (share) |
|---|--:|---|---|
| get | 175 | `<teigeodata_format::sorted::Sorted>::find` 37.6; `<teigeodata_core::bitpack::Packed>::get` 30.3; `<teigeodata_core::bitpack::Packed>::decode_block` 13.4 | `<teigeodata_format::sorted::Sorted>::find` 37.6; `wrapping_add` 12.9; `<teigeodata_core::bitpack::Packed>::get` 8.1 |
| reverse | 6,435 | `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 50.3; `<teigeodata_format::place_index::nearest::Best>::offer` 8.8; `teigeodata_format::place_index::bearing_deg` 8.0 | `push` 12.4; `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 8.9; `get_mut<teigeodata_format::place_index::nearest::Entry>` 6.4 |
| radius | 31,151 | `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 28.6; `teigeodata_format::place_index::bearing_deg` 19.2; `<teigeodata_format::place_index::nearest::Best>::offer` 11.3 | `insert` 7.8; `libm::math::sin::sin` 6.3; `visit` 6.2 |
| admin | 347 | `<teigeodata_core::bitpack::Packed>::get` 54.8; `<teigeodata_format::region::Region>::regions_at` 10.5; `<teigeodata_core::bitpack::Packed>::run` 6.3 | `packed_at` 19.4; `<teigeodata_core::bitpack::Packed>::get` 13.9; `block_words` 10.6 |
| tz | 64 | `<teigeodata_core::bitpack::Packed>::get` 43.3; `<teigeodata_format::region::Region>::regions_at` 15.4; `<teigeodata_engine::Engine>::tz_at` 15.3 | `<teigeodata_core::bitpack::Packed>::get` 16.1; `block_words` 10.6; `_platform_memmove` 7.8 |
| search | 17,193 | `<teigeodata_format::search::fst::Fst>::walk_in::<teigeodata_format::search::fuzzy::Fuzzy, <teigeodata_engine::Engine>::layer_hits::{closure#1}>` 42.5; `<teigeodata_engine::Engine>::search_with` 13.6; `<teigeodata_format::search::fst::Node>::parse` 9.6 | `<teigeodata_format::search::fst::Fst>::walk_in::<teigeodata_format::search::fuzzy::Fuzzy, <teigeodata_engine::Engine>::layer_hits::{closure#1}>` 17.3; `<teigeodata_format::search::fst::Node>::parse` 9.6; `transition` 9.1 |
| open | 368,641 | `<teigeodata_core::bitpack::Packed>::parse` 32.6; `(outside teigeodata)` 22.4; `teigeodata_format::read::check_sections` 12.4 | `__munmap` 21.1; `any<u8, teigeodata_format::read::check_sections::{closure_env#3}>` 12.0; `<teigeodata_core::bitpack::Packed>::parse` 9.7 |
| first | 30,763,525 | `<teigeodata_format::blocks::Blocks>::verify` 55.7; `<teigeodata_engine::layer::Layer>::ensure` 30.2; `(outside teigeodata)` 4.8 | `blake3_hash_many_neon` 83.3; `__munmap` 1.7; `rotate_right<u32>` 1.2 |

### `world-baseline-lite.tgd`

| Suite | ns per call | Top under our frame (share) | Top self time (share) |
|---|--:|---|---|
| get | 223 | `<teigeodata_format::sorted::Sorted>::find` 49.7; `<teigeodata_core::bitpack::Packed>::decode_block` 19.6; `<teigeodata_core::bitpack::Packed>::get` 17.1 | `<teigeodata_format::sorted::Sorted>::find` 48.3; `eq<[u32; 4]>` 14.6; `packed_at` 5.9 |
| reverse | 13,100 | `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 48.6; `teigeodata_format::place_index::bearing_deg` 8.4; `<teigeodata_format::place_index::nearest::Best>::tighten` 8.3 | `push` 8.8; `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 7.1; `visit` 5.7 |
| radius | 3,859 | `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 38.2; `<teigeodata_format::rtree::Tree>::children` 12.6; `<teigeodata_core::bitpack::Packed>::run` 11.5 | `<teigeodata_format::place_index::PlaceIndex>::nearest_within` 12.7; `_platform_memmove` 9.2; `open` 6.4 |
| admin | 1,921 | `<teigeodata_core::bitpack::Packed>::get` 38.4; `<teigeodata_core::bitpack::Packed>::run` 24.2; `<teigeodata_format::region::Region>::regions_at` 15.0 | `packed_at` 20.0; `wrapping_add` 9.1; `<teigeodata_core::bitpack::Packed>::get` 7.8 |
| tz | 761 | `<teigeodata_core::bitpack::Packed>::get` 37.4; `<teigeodata_core::bitpack::Packed>::run` 23.3; `<teigeodata_format::region::Region>::regions_at` 16.5 | `packed_at` 19.0; `<teigeodata_core::bitpack::Packed>::get` 9.0; `<teigeodata_core::bitpack::Packed>::run` 7.9 |
| search | 7,359 | `<teigeodata_format::search::fst::Fst>::walk_in::<teigeodata_format::search::fuzzy::Fuzzy, <teigeodata_engine::Engine>::layer_hits::{closure#1}>` 64.1; `<teigeodata_format::search::fst::Node>::parse` 9.7; `<teigeodata_engine::Engine>::search_with` 5.9 | `<teigeodata_format::search::fst::Fst>::walk_in::<teigeodata_format::search::fuzzy::Fuzzy, <teigeodata_engine::Engine>::layer_hits::{closure#1}>` 24.1; `transition` 14.4; `<teigeodata_format::search::fst::Node>::parse` 9.7 |
| open | 97,153 | `<teigeodata_core::bitpack::Packed>::parse` 40.6; `<teigeodata_engine::layer::Layer>::open::<&alloc::string::String>` 20.6; `(outside teigeodata)` 12.3 | `<teigeodata_core::bitpack::Packed>::parse` 19.7; `__open` 18.0; `__munmap` 10.6 |
| first | 2,645,772 | `<teigeodata_engine::layer::Layer>::ensure` 57.5; `<teigeodata_format::blocks::Blocks>::verify` 36.0; `(outside teigeodata)` 1.4 | `blake3_hash_many_neon` 91.2; `rotate_right<u32>` 1.1; `__open` 1.0 |

Reading them:

- **`get`** is the ID map: `Sorted::find` (interpolation, then the block's values unpacked)
  and the place's columns (`Packed::get`, `decode_block`).
- **`reverse` and `radius`** are the best-first walk of the place index
  (`nearest_within`, its fixed heap's `push` and `pop`), with `bearing_deg` (seven `libm`
  calls per result) the next item.
- **`admin` and `tz`** are random reads of packed columns (`Packed::get`, `run`) inside the
  region index's lookup; each call is a few nanoseconds.
- **`search`** is the Levenshtein automaton's walk of the FST (`walk_in` with `Fuzzy`), as
  measured in [S3](s3-search.md).
- **`open`** is the head checks: the packed columns' directories (`Packed::parse`) and the
  section table; `munmap` is the loop's own unmapping.
- **`first`** is BLAKE3 over the sections the first query reads (D-142), already the SIMD
  path.

## Changes measured A/B

Two binaries, run in turn, best of three one-second rounds at the same load (the
[note](../notes/profiling.md#2-summary-and-recommendation) says why only this comparison holds on a shared
machine). Instruction counts for the same commits come from the `counts` workflow.

| Change | Suite | Nepal lite | World baseline |
|---|---|--:|--:|
| `Sorted::find` interpolates in `u64` (a `u128` division only when the product overflows) | `get` | 117 → 110 ns (−6%) | 230 → 221 ns (−4%) |
| `Packed::parse` checks its directory as independent steps, combined with `&` | `open` | 235 → 201 µs (−14%) | 84 → 70 µs (−17%) |
| Rejected: `Sorted::find` by direct reads instead of unpacking the block | `get` | 116 → 122 ns (+5%) | 228 → 219 ns (−4%) |

## Scaling across threads

```
cargo xtask scaling data/build/pack/np/np-places-lite.tgd --record
```

One `Shared` engine; each thread loads the current snapshot for every query and runs `get`,
`reverse` (10 nearest), `admin_at`, `tz_at` and `search` (10 hits) in turn. Best of three
rounds per thread count. The gate fails below 1.5× at four threads.

| Threads | Nepal lite, queries/s | Speed-up | World baseline, speed-up |
|--:|--:|--:|--:|
| 1 | 154,299 | 1.00× | 1.00× |
| 2 | 274,538 | 1.78× | 2.04× |
| 4 | 552,217 | 3.58× | 3.08× |
| 8 | 690,875 | 4.48× | 6.90× |

Close to linear up to four threads on Nepal; past six threads the work spills onto the
efficiency cores, and other projects' builds held some performance cores throughout. The planted fault (a `Mutex` around every query) gave 0.76× at four threads
and failed the gate. The Nepal table is the site's
[measured page](../../../site/data/measured/scaling.json) record.
