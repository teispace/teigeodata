---
title: "Profiling the query and load suites"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-05-15
---

# Profiling the query and load suites

## 1. Question

Phase 8.3 asks for a sampling profile of each Q and L suite
([benchmarks](../../06-engineering/benchmarks.md)) on Nepal's pack and the world baseline, the
three largest self-time items per suite, and a change only where a profile justifies it. The
plan named `samply`. Which profiler works on the maintainer's machine with nothing installed,
how is a suite isolated so that every sample belongs to it, and how are two builds compared on
a machine shared with other projects?

## 2. Summary and recommendation

- **Profiler: Instruments' Time Profiler through `xctrace`**, which ships with Xcode's command
  line tools (27.2 here). `samply` and `perf` are not installed, and the standing rule is no
  local installs. `xctrace record --template 'Time Profiler' --launch` samples at 1 kHz;
  `xctrace export --xpath …time-profile` gives every sample's backtrace as XML.
- **One suite per process.** The engine's `profile` example runs one query kind in a loop for
  a fixed time (or a fixed number of calls), after one warm-up query of each kind, so the
  profile holds that suite alone.
- **`cargo xtask profile`** builds the example with line tables
  (`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`, so inlined frames keep their names),
  records each suite, and reports two tables per suite: self time by leaf frame, and time
  under the nearest teigeodata frame (where in our code the time goes, whatever library it
  ends in). Symbols are demangled with `rustc-demangle`.
- **Comparing builds: wall-clock, A/B, alternating.** Both binaries are kept, and run in turn,
  three rounds of one second each, at the same load. On this machine, runs of 2 s or more
  drift (a long run is moved to an efficiency core under load: the same binary gave 116 ns per
  `get` at 1 s and 228 ns at 2 s), so only alternating one-second rounds are compared.
- **Instruction counts come from CI**, the `counts` workflow (Callgrind through `gungraun`,
  the load-independent gate). macOS's `/usr/bin/time -l` reports "instructions retired", but
  its counts were not usable: 2 million lookups on the world baseline added about a million
  instructions, under one per lookup, so the counter misses most of the run.

## 3. Findings

Profiles of 2026-11-15, 6 s per suite, load average 6 to 8 (other projects building). Nepal
lite (`np-places-lite.tgd`, 87.5 MB) and the world baseline (`world-baseline-lite.tgd`,
4.9 MB). The full tables are in [benches/phase-8](../benches/phase-8.md).

| Suite | Largest items under our frames | Verdict |
|---|---|---|
| Q7 `get` | `Sorted::find` 40–50%, block decode 15–19%; on the world baseline a `u128` division (`u128_div_rem`, 4.4%) in the interpolation step | **Changed**: the interpolation in `u64` |
| L1 `open` | `Packed::parse` 41–47%, of which the directory check (line 242, a running sum over every block) is most | **Changed**: a check of independent steps, which vectorises |
| Q2 `admin`, Q3 `tz` | `Packed::get` 37–54%, `Packed::run` 7–24%, `Region::regions_at`, `crosses` | Kept: random access to packed columns is the design; each call is a few ns |
| Q1 `reverse`, Q8 `radius` | `nearest_within` 30–52% (its heap's push and pop), `Best::offer` 10–16%, `bearing_deg` 8–19% (seven `libm` calls per result) | Kept: see §4 |
| Q6 `search` | the fuzzy automaton's walk of the FST 41–64%, `Node::parse` 10% | Kept: the cost of the Levenshtein walk, measured in S3 |
| L4 `first` | `blake3_hash_many_neon` 87–89% | Kept: the first query proves the sections it reads (D-142); BLAKE3 is already SIMD |

## 4. Options compared

Each candidate measured A/B as in §2, three alternating one-second rounds, ns per call.

| Change | Nepal before → after | World before → after | Verdict |
|---|---|---|---|
| `Sorted::find`: interpolation in `u64`, `u128` only when the product overflows | 117 → 110 (−6%) | 230 → 221 (−4%) | **Taken** |
| `Sorted::find`: binary search by direct reads (seven `Packed::get`) instead of unpacking the block, for FOR and ramp columns | 116 → 122 (+5%) | 228 → 219 (−4%) | Rejected: slower on the larger pack; unpacking 128 values with the vector code costs less than seven scattered reads with their bounds checks |
| `Packed::parse`: the directory check as independent steps (`offset[i] + 4·width[i] = offset[i+1]`, first offset 0, every width ≤ 32), combined with `&` so the loop has no early exit | 235 → 201 µs (−14%) | 84 → 70 µs (−17%) | **Taken** (the final form, best of three alternating rounds; a first form with an early exit gave −10% and −13%); equivalent by induction to the running sum, with three new rejection cases, each proved to fail when its rule is removed |
| `bearing_deg`: the query point's sine and cosine once per query, not once per result | n/a | n/a | Not taken: at most 2% of `reverse`, for a second bearing function to keep equal on every platform |
| `reverse`: a heap of indices into an arena instead of 40-byte entries | n/a | n/a | Not taken now: the heap is fixed-size and allocation-free; an idea for later ([ideas](../../01-product/ideas.md)) |

## 5. What we build ourselves, and why

`cargo xtask profile` and the `profile` example: about 300 lines that make the profile
repeatable by anyone with a Mac, so a later change is justified the same way. The export
parser keeps a table of each `id` and follows each `ref`, as Instruments writes each frame,
backtrace and weight once. A unit test with refs at every level holds it, and was proved to
fail with the backtrace lookup broken.

## 6. Impact on the plan

- Phase 8.3's first item: Instruments in place of `samply` (D-338). The three largest items
  per suite are on [benches/phase-8](../benches/phase-8.md).
- The two changes are recorded there with their wall-clock before and after; their
  instruction counts come from the `counts` workflow's run on the commit.

## 7. Risks and open items

- Instruments is macOS only. A Linux contributor uses `perf` or `samply` with the same
  example; the example is the part that matters.
- A trace names the machine and its owner's device name: traces stay in `target/profile/` and
  are never committed.
- Wall-clock on a shared machine: every figure here is an alternating A/B at the same load,
  never compared across sittings.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Instruments / `xctrace` | 27.2 (27B52) | 2026-11-15 | `xctrace version` |
| `rustc-demangle` | 0.1.28 | 2026-11-15 | crates.io |

## 9. Sources

- `xctrace` manual page (`man xctrace`), Xcode 27.2, read 2026-11-15.
- [rustc-demangle](https://crates.io/crates/rustc-demangle), crates.io, accessed 2026-11-15.
- [Benchmarks](../../06-engineering/benchmarks.md), the Q and L suites.
