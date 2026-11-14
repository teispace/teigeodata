---
title: "One parity suite, proven to fail on a planted difference"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# One parity suite, proven to fail on a planted difference

## 1. Question

Phase 5.9 asks for one parity suite of golden vectors, run in every language in CI and proven
to fail on a planted difference. Every binding already reads `bindings/parity.json` (checked
against Rust by `teigeodata-api`'s tests), except C and C++, which check literals. What does
"proven to fail" take, how much of the file must each language check, and how is that kept
true as the file grows?

## 2. Summary and recommendation

- **Mutation of the expectations, not of the code.** For each leaf of `parity.json` (181 of
  them, 2026-11-14, leaving out `about` notes), write a copy with that one value changed and run
  each language's suite against it: the suite must fail. A leaf that survives in a language is
  a value that language does not check. This is mutation testing (cargo-mutants, Stryker,
  PIT) aimed at the oracle instead of the program, and it measures exactly the claim "every
  language checks the same golden vectors".
- **One switch in every suite:** the environment variable `TEIGEODATA_PARITY` names the file
  (default `bindings/parity.json`). The planted copy is written next to the original, one per
  language (`bindings/parity.planted.<lang>.json`, ignored by git), so that fixture paths,
  which are relative to the file, still resolve.
- **C and C++ join the suite** through a header generated from `parity.json`
  (`bindings/c/parity_data.h`, kept current by `cargo xtask bindings --check`): one macro per
  leaf, plus `_COUNT` and `_ITEMS` (an initializer list) per array. The planted run copies the
  source next to a planted header in `target/parity-<lang>/` (a quoted include searches the
  source's own directory first) and recompiles with the same `-Werror` flags; a compile
  error is reported as a problem, never counted as a killed mutant.
- **`cargo xtask parity-mutants [--lang …] [--leaf …]`** runs it and prints the survivors.
  Survivors that cannot be killed (an input whose change does not alter a result, such as the
  refused mirror's URL) are listed in the task with the reason; any other survivor fails.
- **Where it runs:** every leaf times every language is about 1,300 suite runs; it runs as a
  scheduled and on-demand workflow, one job per language, so it never blocks a push (D-080).
  `fast-check` plants a fixed handful of leaves (one per section) in every language, which
  proves on each push that no suite has stopped reading the file.

## 3. Findings

- **The suites today (2026-11-14):** Python, Node.js, Dart and Go read `../parity.json` by a
  fixed relative path; the JVM and Swift take the path as an argument and resolve fixtures
  against its directory; .NET searches upward for it (D-298); C and C++ hard-code the values
  (`bindings/c/parity.c`, 291 lines, `parity.cpp`, 200).
- **Leaves:** 188 scalars, 7 of them `about` notes. About a quarter are inputs (queries,
  points, limits) rather than results; changing an input usually changes a checked result,
  so it is killed too, but not always (a mirror that refuses connections refuses at any
  port).
- **Mutation operators** that cannot accidentally still pass: a string gains a character; an
  integer gains 1; a float moves by `max(1, |x|)`; a boolean flips. Tolerances in the suites
  are 1e-6 m (distance) and 0.1 m (first reverse distance), far below these moves.
- **cargo-mutants** (the Rust tool) mutates code, which `cargo-mutants` already could do for
  the engine; it does not reach the bindings' oracles. **Stryker, PIT** are per-language and
  likewise mutate code: eight tools, eight configurations, and none answers whether the
  languages check the same vectors.

- **The first full run (2026-11-14)** found every suite short of the file: 58 survivors in
  Node.js, Dart and Swift, 57 in the JVM, 45 in Python, 43 in Go (out of 181). The shared ones
  were inputs; the rest were real gaps: the column names of `get` and `sample_rows`, `get`'s
  longitude, the last sample name, the zone offset's `dst`, and the numeric error codes.
  C and C++, rewritten against the header, survived only the 32 shared leaves.
- **The string operator matters.** Appending `~` let `fold` inputs and search queries survive
  (`fold` drops punctuation, search forgives one character). Reversing the string and adding a
  letter kills them, and changes `""` too. With it, 23 leaves survive in every language; each
  is an input that cannot change any result, listed in the task with its reason (D-300).
- **Run time:** the Dart suite took 22.7 s a run, all of it a test mirror that kept trickling
  after its job was cancelled; stopped at teardown it takes 0.9 s. A full run of the eight
  languages, three at a time, takes 318 s on this machine including the builds.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Mutate each leaf of `parity.json`, run every suite | One tool for eight languages; measures the exact claim; finds unchecked values | ~1,300 runs: scheduled, not per push | ours | new | **Chosen** |
| A few hand-planted faults per language | Cheap; already done for Go and .NET | Proves the suite runs, not that it checks every value | ours | in use | Kept as the `fast-check` sample |
| Per-language code mutation (cargo-mutants, Stryker, PIT, …) | Finds weak tests of each binding's code | Eight tools; does not compare languages | MIT / Apache-2.0 | mature | Not for 5.9 |
| Suites report the keys they read | One run | Every suite instrumented; typed decoders (Go, .NET) read keys they never compare | ours | — | Rejected |

## 5. What we build ourselves, and why

`cargo xtask parity-mutants`: enumerates the leaves, plants one change at a time, runs the
suites, and reports per language the leaves that survived. Its own gate: with an unmutated
copy every suite must pass (a suite that fails on the original would make every mutant
"killed"), and a planted survivor (a language told to skip a leaf) must be reported.

## 6. Impact on the plan

- 5.9's first item: the switch in every suite, the C/C++ header, the task, the workflow and
  the `fast-check` sample; survivors fixed by adding the missing checks.
- The benchmarks item is separate; .NET is not part of either while it is parked (D-299).

## 7. Risks and open items

- If a language's run grows long, shard it by leaf (`--leaf <prefix>`).
- A leaf that is legitimately unchecked in one language (a platform difference) needs a
  reason in the allow-list; the list must stay short and is reviewed with the file.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| cargo-mutants | 27.1.0 (2026-06-02) | 2026-11-14 | crates.io API (for comparison only) |

## 9. Sources

- [cargo-mutants](https://mutants.rs/), Martin Pool, accessed 2026-11-14.
- [Stryker Mutator](https://stryker-mutator.io/), accessed 2026-11-14.
- [PIT mutation testing](https://pitest.org/), accessed 2026-11-14.
- Jia and Harman, "An Analysis and Survey of the Development of Mutation Testing", IEEE TSE
  37(5), 2011.
