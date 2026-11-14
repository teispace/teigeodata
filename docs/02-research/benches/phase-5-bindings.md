---
title: "Phase 5 binding benchmarks: call cost, a 1,000-row query and its decoding per language"
status: research
updated: 2026-11-14
---

# Phase 5 binding benchmarks

**Question (5.9):** what does each language add to the Rust engine for a call, a string, a
callback, and a real 1,000-row query decoded into the language's own strings and numbers? The
method is the [S4 benchmark](s4-bindings.md)'s, extended by one case; .NET is left out while
it is parked (D-299).

**Answer:** on a 1,000-row query the binding is a small part of the cost. The engine's query
takes 657 µs in Rust. Every language but Python adds 3 to 13 % on top (18 to 82 µs), and Python
adds 33 % (217 µs). Python's extra cost is decoding 1,000 names into `str` objects. The per-call
costs are those of S4 within a few ns, plus Go, which is new.

## Method

`bindings/bench.sh 3`, 2026-11-14, M2 Pro, macOS. The machine was shared: load averages 10–17
on 10 cores, from another project's build and Spotlight indexing. Each run takes the best of 5
rounds, and the table shows the best of the 3 runs. Runtimes: Swift 6.4.2, Dart 3.14.0,
Node.js 26.13.1, Java 25.0.1, Python 3.15.2, Go 1.27.4. Each cell is ns per call; the callback
column is ns per callback within one `progress(1000)` call.

The fifth case, `radius(1000)+decode`, runs on the Nepal pack of a local data build
(`np-places-lite.tgd`, 87.5 MB), which `TEIGEODATA_BENCH_PACK` names:

- the query is `within_radius(27.7, 85.3, 50 km, 1000)`: places within 50 km of Kathmandu,
  nearest first, 1,000 rows, a blob of 83,008 bytes with 12 columns;
- the decoding turns every row's `name` into the language's own string (Rust `&str`, C pointer
  and length, Swift `String`, Dart `String`, a JS string, `java.lang.String`, `str`, a Go
  `string`) and sums the `lat` column;
- Python and Go use their package's blob reader (`teigeodata.blob.Reader`, `tg.Reader`); the
  others decode the documented format directly, as their parity suites do.

Text search is not used for the 1,000 rows: it returns at most a few dozen best matches (66 for
`tole` on this pack), whatever the limit.

## Results

| | noop | fold("Kathmandu") | sample_rows(1000) + sum | callback | radius(1000) + decode |
|---|---:|---:|---:|---:|---:|
| Rust, direct (the floor) | 0.0 | 34.9 | 19,398 | 0.3 | 657 µs |
| C ABI, from Rust | 0.3 | 42.1 | 19,110 | 0.9 | |
| C | 3.6 | 45.7 | 17,733 | 1.2 | 688 µs |
| Swift | 1.8 | 57.2 | 17,824 | 5.4 | 710 µs |
| Dart (AOT) | 5.8 | 76.3 | 19,658 | 27.3 | 680 µs |
| Node.js | 17.2 | 224.6 | 22,642 | 59.2 | 725 µs |
| JVM | 25.5 | 486.3 | 19,564 | 226.3 | 675 µs |
| Python | 24.5 | 96.4 | 30,067 | 23.7 | 874 µs |
| Go, cgo | 25.9 | 102.9 | 24,733 | 47.0 | 723 µs |
| Go, purego | 188.0 | 762.4 | 25,855 | 220.7 | 739 µs |

## Findings

- **The engine sets the cost of a real query, not the binding.** On 1,000 rows the languages
  sit between 675 and 739 µs against 657 µs in Rust, except Python at 874 µs. Building 1,000
  Python `str` objects from the blob is what Python adds. A caller that needs only some columns
  or some rows pays less, because the reader decodes on demand.
- **The query itself is 0.66 µs per row.** That is the engine's spatial search, ranking and
  blob encoding of 12 columns. It belongs to the engine's benchmarks
  ([phase 4](phase-4.md)), not to the bindings.
- **Go through purego costs about 160 ns more per call than through cgo** (noop 188 ns against
  26 ns), from its generic call path. On the 1,000-row query that is 2 %. This is why cgo is
  the default and purego the fallback without a C toolchain (D-296).
- **The per-call costs match S4** (2026-11-07) within a few ns. Python's callback fell from
  44.8 ns to 23.7 ns since then.
- **Strings into the JVM and Node still cost the most** (486 ns and 225 ns for `fold`), as in S4.
  Results come as blobs, so this matters only for calls that take or return a single string.

## Reproduce

```sh
TEIGEODATA_BENCH_PACK=data/build/pack/np/np-places-lite.tgd bindings/bench.sh 3
```

Without a pack, the first four cases run and the fifth is skipped with a note.
