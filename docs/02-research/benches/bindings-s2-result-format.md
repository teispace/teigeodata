---
title: "Bindings spike S2: the result format"
status: research
updated: 2026-11-14
---

# Bindings spike S2: the result format

**Question:** should query results cross the boundary as our blob (v0, D-068), Arrow (IPC or
the C Data interface) or FlatBuffers? Planned in the [bindings note](../notes/bindings-and-mcp.md)
§7 and [phase 5](../../07-planning/phases/phase-5.md) step 5.0.

**Answer: keep our own blob, as v1.** On a real 10,000-row result it is as small and as fast
to read as Arrow IPC, and adds 16.5 KB to a native library where Arrow adds 1.33 MB (1.66 MB to
WASM), with no Arrow library needed in any language. FlatBuffers rows are 33 % larger and 4.6×
slower to read. The spike also shows what v0 lacks: nulls, and dictionaries for repeated
strings, which are most of a result's bytes and of every language's decoding time (D-275).

## Method

- **Data:** the first 10,000 places of the released baseline layer (`tgd export`, data-2026.11.14.3),
  ten columns as a search result carries them: `id`, `name`, `kind`, `level`, `lat`, `lon`,
  `elevation` (null for 33 %), `parent`, `tz`, `iso` (nullable).
- **Formats:** blob v0 (nulls as `i32::MIN` and empty strings, since v0 has none); Arrow IPC
  stream with arrow-rs 60.1.0 (`default-features = false`); FlatBuffers 25.12.19, one table per
  row with a verifier as generated code would write it.
- **Reading** means verifying the buffer and touching every value: the sum of the coordinates
  and elevations, the null count and the string bytes. All three give the same digest.
- **Probe:** `tools/result-format-probe` (`bench` binary, `read.mjs`, `read.py`); release,
  fat LTO, one codegen unit; medians of 41 runs (61 in Node, 21 in Python). Apple M2 Pro,
  Rust 1.99.1, Node 26.13.1, Python 3.15.2. 2026-11-14.

## Results

**Rust: size and time**

| Format | Bytes | Encode µs | Verify and read all µs | ns per row |
|---|--:|--:|--:|--:|
| blob v0 | 917,936 | 309 | 283 | 28.3 |
| Arrow IPC | 924,360 | 799 | 225 | 22.5 |
| FlatBuffers rows | 1,217,528 | 1,047 | 1,313 | 131.3 |
| **blob v1** (built after the decision) | **584,360** | 664 | 290 | 29.0 |

Blob v1 is the same result as the API will send it: `parent`, `tz` and `iso` as dictionaries,
real nulls, `kind` and `level` as `u8`. It is 36 % smaller than v0 and Arrow IPC and reads as
fast as v0. Its encoder finds repeats with an open-addressing table and a word-at-a-time hash:
a `BTreeMap` took 1,142 µs. With v1 the reader adds 16,592 B native and 19,358 B to WASM.

Blob v0 reads slower than Arrow IPC only because it validates each string's UTF-8 as it is read;
both are within the noise of a 10,000-row search.

**What each adds to a library** (stripped `cdylib`, encoder and reader, over an empty build of
302,000 B native and 17,906 B WASM):

| Format | macOS arm64 | `wasm32-unknown-unknown` |
|---|--:|--:|
| blob | +16,544 B | +12,940 B |
| Arrow IPC | +1,326,608 B | +1,656,242 B |
| FlatBuffers | +33,136 B | +38,710 B |

A language reading Arrow needs an Arrow library as well: `pyarrow` 26.0.1 is a 37 MB wheel
(macOS arm64), `apache-arrow` 21.2.0 is 5.8 MB unpacked on npm. The C Data interface avoids the
IPC encoding but not those libraries, and adds a release callback per array, called from
whichever thread frees it: against the threading contract (D-274).

**In the languages** (blob v0 against JSON of the same rows, 1,967,763 B):

| Runtime | Numbers only, in place | Every value | JSON parse and read |
|---|--:|--:|--:|
| Node 26 | 29 µs | 5,502 µs (`TextDecoder` per string); 2,241 µs (`utf8Slice`) | 6,133 µs |
| Python 3.15 | 341 µs | 8,668 µs | 16,015 µs |

Numbers cost almost nothing: the columns are typed-array and `memoryview` views. **Strings are
the whole cost**, about 45 ns each in Node and 160 ns in Python, so bindings decode a string
only when it is read, and the format should send each distinct string once.

**Repeated strings** (bytes of the column, plain against a dictionary with `u16` codes):

| Column | Distinct | Plain B | Dictionary B |
|---|--:|--:|--:|
| `tz` | 160 | 193,853 | 23,238 |
| `kind` (as text) | 4 | 133,310 | 20,055 |
| `parent` | 1,197 | 128,580 | 35,209 |
| `iso` | 1,348 | 46,529 | 31,921 |
| `name` | 9,090 | 137,635 | 146,620 |

A dictionary saves 28 % of this result and leaves 160 timezone strings to decode instead of
10,000. Names and IDs stay plain. In the real API `kind` is an enum and `parent` an integer ID,
so neither is text.

## Decision

D-275: blob v1, still our own format, read in place in every language:
1. **Validity:** a column may carry a null bitmap, one bit per row, least significant bit first
   (as in Arrow); its offset goes in the descriptor's reserved field, 0 when no row is null.
2. **Dictionary strings:** a type `Utf8Dict`, with row codes (`u16`, or `u32` past 65,535
   distinct values) into a dictionary stored as an ordinary UTF-8 column.
3. **Narrow integers:** `u8`, `i16` and `u16`, for enums, levels and elevations.
4. **Evolution stays by name:** readers look columns up by name, ignore unknown ones and treat a
   missing one as absent; only a layout change bumps the magic (`TGB1`).

Bindings expose numbers as views and strings lazily (decoded when read, a dictionary once).

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-14 | Blob v1 measured: 584,360 B, 664 µs to encode | the decision built |
| 2026-11-14 | First version | phase 5.0, S2 |
