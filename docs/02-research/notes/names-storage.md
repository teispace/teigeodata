---
title: "Name text storage with random access (B3, B4)"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# Name text storage with random access (B3, B4)

## 1. Question

How do the NAMES section (kind 4: per-place name records with language, script, name type and
the UTF-8 text) and the BLOBS section (kind 14: cold data such as attribution text and long
strings) store text compactly, while one place's names still decode fast?

This feeds Phase 1 step 1.4, items B3 and B4
([phase 1](../../07-planning/phases/phase-1.md)), and the NAMES and BLOBS rows of the
[pack format](../../04-architecture/pack-format.md#sections). It builds on
[format, compression and indexing](format-compression-indexing.md) §3.2 and spike
[S1](../benches/s1-codecs.md).

## 2. Summary and recommendation

- **NAMES text: FSST with one symbol table per script group, and our own safe decoder.**
  On Nepal it costs **8.49 B per place** for text plus offsets (0.665 of the raw text). One place's
  names decode in **20 ns**, and all names decode at about **1.07 GB/s**. Zstd blocks with a
  trained dictionary are 34% smaller (5.62 B per place) but cost **4.6 µs per place** with
  `ruzstd`, 230 times slower. That is too slow for a section that every search result, reverse
  lookup and hierarchy line reads.
- **Per-script tables are required, not optional.** A single table trained on mostly Latin text
  *expands* Devanagari names to 1.86 times their size (nearly every byte is escaped). A Devanagari
  table compresses them to 0.31. GeoNames Nepal has only 176 Devanagari names, so the size of this
  effect on the real mixed-script consensus data is **unverified**. The direction is not in doubt.
- **Record layout:** NAMES is a [column table](../../04-architecture/pack-format.md#column-tables-encoding-1)
  of integer columns plus one FSST text heap. A place's names are records
  `place_first[row] .. place_first[row + 1]`. Each record has a language, script, type and text
  offset. Records inside a place are sorted by display priority, so the display name is record
  `place_first[row]`, read in O(1).
- **No blocks in NAMES.** FSST compresses each string on its own, so the block size is one string.
  The text section keeps a codec byte so that a cold region (zstd blocks of at least 64 names, 16 KB
  dictionary) can be added later. That should happen only if consensus data, with many
  `name:xx` languages per city, shows that the cold names dominate. That is **unverified**.
- **BLOBS: zstd frames, encoded with C zstd in the builder, decoded with `ruzstd`.** One frame per
  blob, or frames of about 64 KB for many small blobs, with a trained dictionary when the blobs are
  small. BLOBS is read rarely, for example attribution at open. `ruzstd` decodes large frames at
  about 350 MB/s, against about 1.1 GB/s for C zstd, which is enough.

## 3. Findings

### 3.1 Corpus

GeoNames `NP.txt` (the S1 export of 2026-11-06). Places are sorted in Hilbert order on a 2^16 grid,
as pack rows are. Each place's names are `name`, `asciiname` and `alternatenames`, deduplicated in
that order.

| measure | value |
|---|---:|
| places | 148,210 |
| names (deduplicated) | 183,248 |
| raw UTF-8 bytes | 1,892,140 (10.3 B per name) |
| Latin / Devanagari / other names | 182,389 / 176 / 683 |
| places with more than one name | 28,263; the maximum is 186 names on one place |

S1 counted 210,866 names because it did not deduplicate: GeoNames repeats the primary name in
`alternatenames`. The counts here are the ones a pack would store.

### 3.2 Measurements (2026-11-07)

Apple silicon laptop, shared, load about 8 to 9 during the run. Release build, single thread,
`std::time::Instant`. "One place" is the mean time over 300,000 uniformly random places to locate
and decode all of that place's names. "All names" is the decoded output rate over every record.
Two runs at load 5 to 9 agreed within 10%. A third run at load 21 was discarded. Offsets are sized
with the [bitpack](../../04-architecture/pack-format.md#column-tables-encoding-1) model: FOR or ramp
per 128-value block, with the 9-byte block header. Block options also count their block-offset and
block-first-place columns. lz4 also counts a decoded-length column. Peak memory of the whole
benchmark was 92 MB.

| option | text B | offsets B | table or dict B | total B | B per place | vs raw text | one place ns | all names MB/s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| plain UTF-8 + offsets (baseline) | 1,892,140 | 262,304 | 0 | 2,154,444 | 14.54 | 1.139 | 34 | 927 |
| FSST, one table, simple decoder | 1,034,553 | 236,160 | 854 | 1,271,567 | 8.58 | 0.672 | 69 | 265 |
| FSST, one table, 8-byte-store decoder | 1,034,553 | 236,160 | 854 | 1,271,567 | 8.58 | 0.672 | 21 | 1,061 |
| **FSST, per-script tables, 8-byte-store decoder** | 1,019,373 | 236,912 | 2,396 | **1,258,681** | **8.49** | **0.665** | **20** | **1,074** |
| zstd-19 blocks ≥16 names, no dict, `ruzstd` | 1,504,436 | 37,840 | 0 | 1,542,276 | 10.41 | 0.815 | 3,583 | 50 |
| zstd-19 blocks ≥16 names, no dict, C zstd | 〃 | 〃 | 〃 | 〃 | 10.41 | 0.815 | 2,031 | 90 |
| zstd-19 blocks ≥64 names, no dict, `ruzstd` | 1,108,934 | 11,040 | 0 | 1,119,974 | 7.56 | 0.592 | 7,170 | 100 |
| zstd-19 blocks ≥64 names, no dict, C zstd | 〃 | 〃 | 〃 | 〃 | 7.56 | 0.592 | 3,183 | 241 |
| zstd-19 blocks ≥16 names, 16 KB dict, `ruzstd` | 1,008,630 | 36,784 | 16,384 | 1,061,798 | 7.16 | 0.561 | 1,751 | 111 |
| zstd-19 blocks ≥16 names, 16 KB dict, C zstd | 〃 | 〃 | 〃 | 〃 | 7.16 | 0.561 | 599 | 359 |
| zstd-19 blocks ≥64 names, 16 KB dict, `ruzstd` | 805,466 | 11,040 | 16,384 | 832,890 | 5.62 | 0.440 | 4,644 | 165 |
| zstd-19 blocks ≥64 names, 16 KB dict, C zstd | 〃 | 〃 | 〃 | 〃 | 5.62 | 0.440 | 1,714 | 492 |
| zstd-19 blocks ≥64 names, 64 KB dict, `ruzstd` | 743,418 | 10,864 | 65,536 | 819,818 | 5.53 | 0.433 | 5,455 | 131 |
| zstd-19 blocks ≥64 names, 64 KB dict, C zstd | 〃 | 〃 | 〃 | 〃 | 5.53 | 0.433 | 1,711 | 494 |
| lz4_flex blocks ≥16 names | 1,680,962 | 51,040 | 0 | 1,732,002 | 11.69 | 0.915 | 241 | 1,650 |
| lz4_flex blocks ≥64 names | 1,480,946 | 15,128 | 0 | 1,496,074 | 10.09 | 0.791 | 655 | 2,298 |
| lz4_flex blocks ≥16 names, 16 KB dict | 1,342,721 | 49,792 | 16,384 | 1,408,897 | 9.51 | 0.745 | 311 | 975 |
| lz4_flex blocks ≥64 names, 16 KB dict | 1,251,894 | 14,888 | 16,384 | 1,283,166 | 8.66 | 0.678 | 1,077 | 1,054 |

Other measurements from the same runs:

- **Hybrid:** FSST for each place's first (display) name and zstd-19 blocks of at least 64
  alternate names with a 16 KB dictionary. It totals about 1,163,424 B, or **7.85 B per place**:
  8% smaller than FSST for every name. The display name decodes in 15 ns. A place with alternates
  (19% of places) also pays a block decode of about 4.6 µs.
- **Offsets column:** the raw-text offsets cost 262,304 B with FOR or ramp coding. With delta coding
  (the deltas are the string lengths) they cost 153,376 B, 42% less. The price is that reading a
  delta-coded offset unpacks its 128-value block and sums the deltas.
- **Record columns, the same for every option:** `place_first` costs 106,368 B (0.72 B per place).
  A 2-bit script column costs 16,512 B.
- **One frame of all names (the BLOBS throughput case):** zstd-19 gives 610,952 B. `ruzstd` decodes
  it at 353 MB/s and C zstd at 1,103 MB/s. lz4 on the same frame gives 1,134,803 B.
- **FSST per-script ratios:** Latin 0.539, Devanagari 0.306, other scripts 0.653. With one shared
  table, Devanagari is 1.862.

### 3.3 What the numbers say

1. **FSST is the only option that is both small and fast per place.** It is 41% smaller than the
   baseline and as fast to read: 20 ns against 34 ns, because it reads fewer bytes. lz4 is never
   better. Without a dictionary it is larger than FSST and 12 to 30 times slower per place. With a
   dictionary it matches FSST's size and is still 15 to 50 times slower.
2. **Zstd with a dictionary is the size winner, and the cost is a block decode per access.** The
   dictionary matters a lot: blocks of at least 16 names shrink from 10.41 to 7.16 B per place. A
   64 KB dictionary adds almost nothing over 16 KB. `ruzstd` is 2.7 to 3.5 times slower than C zstd
   on small dictionary frames, because it copies the dictionary into its window and rebuilds its
   tables on every frame (`FrameDecoder::force_dict` calls `init_from_dict`). Even C zstd needs
   1.7 µs per place at 64 names.
3. **The query budget decides.** Search answers in under 100 µs
   ([names and search](names-and-search.md)) and shows about 10 to 20 results. Each result shows a
   name plus its admin chain of about 3 to 5 places. That is 30 to 100 name lookups per query:
   2 µs with FSST, and 140 to 460 µs with `ruzstd` blocks. Reverse geocoding a batch of points
   behaves the same way.
4. **The decoder shape matters 3.5 times.** A safe FSST decoder that writes all 8 symbol bytes
   unconditionally into a buffer with 8 bytes of slack, then advances by the symbol's length, runs
   at 21 ns. A `Vec::extend_from_slice` per symbol runs at 69 ns. Both are plain safe Rust.
5. **Offsets are now 19% of the FSST total.** That is 1.29 B per name with FOR coding. Delta coding
   would cut about 40% of it, at the cost of a block unpack per access. B1's
   `bitpack::encode_auto` already picks delta only when it is at least 10% smaller. Whether the
   unpack cost is acceptable on the hot path is for the implementation step to measure.

### 3.4 Crate facts (checked 2026-11-07, crates.io API and the published sources)

- **`fsst-rs` 0.7.0** (2026-10-27, Apache-2.0, MSRV 1.86, library name `fsst`):
  - It uses `unsafe` throughout `compress_into`, `compress_word` and `decompress_into`. It has no
    `no_std` attribute.
  - Training samples only 16 KB (`FSST_SAMPLETARGET = 1 << 14`). Training on our corpus took 1 to
    3 ms.
  - Retraining on the same input gave the identical symbol table, so it is deterministic on this
    corpus. That was checked once, not proven.
  - Its `Symbol::to_u64` and `symbol_lengths` are enough to export the table into our own format.
    Our safe decoder round-tripped every one of the 183,248 names.
- **`ruzstd` 0.9.1** (2026-10-11, MIT, MSRV 1.87): `#![no_std]` with `alloc`.
  - It is not `forbid(unsafe_code)`: `decoding/ringbuffer.rs` has 38 `unsafe` uses and
    `decode_buffer.rs` has 2.
  - Frames written without a dictionary ID need `reset` then `force_dict`.
  - It round-tripped every block in this benchmark.
- **`zstd` 0.14.3** (2026-10-16, BSD-3-Clause; `zstd-sys` 2.1.2+zstd.1.5.7): C bindings, used only
  in the builder. `zstd::dict::from_samples` trains the dictionary. `include_dictid(false)` and
  `include_checksum(false)` trim each frame; the 4-byte magic number stays.
- **`lz4_flex` 0.14.0** (2026-07-14, MIT, MSRV 1.81): `no_std` for the block format, and it carries
  `forbid(unsafe_code)` when `safe-encode` and `safe-decode` are both on. Its block format has no
  header, so the decoded length must be stored separately.
- **Other FSST crates:** `fsst` 14.0.2 (Lance, MSRV 1.94), `vortex-fsst` 0.93.0 (2026-11-06,
  Vortex arrays), `omnizip-fsst` 0.21.117, `jdb_fsst` 0.1.4, `fsst-rust` 0.1.1 (2024). None of them
  is a `no_std`, `unsafe`-free decoder we could depend on. Their safety status was taken from their
  descriptions only and is **unverified** beyond `fsst-rs`.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| FSST, per-script tables, our decoder | 20 ns per place, 0.665 of raw, independent strings, decoder of about 40 lines, `no_std`, safe | Offsets per name (1.29 B); about 0.8 KB per table; needs a trainer in the builder | ours; `fsst-rs` Apache-2.0 in the builder only | paper 2020; `fsst-rs` 0.7.0 | **Adopt for NAMES** |
| zstd blocks + dictionary, `ruzstd` | Smallest with random access (0.440 of raw), one codec shared with BLOBS | 4.6 µs per place, `ruzstd` has internal `unsafe`, about 2.7× slower than C | MIT / BSD-3 | `ruzstd` 0.9.1, `zstd` 0.14.3 | Adopt for BLOBS; reserve for a cold NAMES region |
| lz4_flex blocks | Fastest bulk decode (2.3 GB/s), `forbid(unsafe)` | Larger than FSST without a dictionary, 12 to 50× slower per place | MIT | 0.14.0 | Reject |
| Plain UTF-8 + offsets | Simplest; zero decode | 1.71× the size of FSST | — | — | Baseline only |
| Hybrid: FSST display names + zstd alternates | 7.85 B per place (−8%), display name 15 ns | Two codecs in one section; 4.6 µs whenever alternates are read | as above | — | Revisit on consensus data |

## 5. What we build ourselves, and why

- **An FSST decoder in `teigeodata-core`.** It is `no_std`, has no `unsafe` and allocates nothing:
  - It decodes into a caller buffer of at least `8 × encoded_len + 8` bytes. The encoded length
    bounds the output, so the bound is checked up front, never per byte.
  - The table holds 255 `u64` symbols and their lengths, parsed and validated once at open: each
    length is 1 to 8, and the bytes past a symbol's length are zero.
  - Code 255 escapes one literal byte. A truncated escape at the end of a string is a format error
    found by the open-time deep check, and the reader never panics on it.
  - Its gate is to round-trip every string of the corpus against `fsst-rs` used as the oracle, and
    to decode one place in 25 ns or less on this machine class.
- **The trainer stays in the builder.** It is either `fsst-rs`, with its determinism checked by
  training twice in CI, or our own implementation of the paper's algorithm with a larger sample. The
  16 KB sample in `fsst-rs` leaves room: an A/B of the sample size against the Latin ratio of 0.539
  is worth one measurement at build time.
- **Script groups.** The table index of a record comes from its script through a small map in the
  section header, for example Latin → 0, Devanagari → 1, other → 2. The forge trains one table per
  script group that has enough text, and sends the long tail to a shared fallback table.

## 6. Impact on the plan

- **[Pack format](../../04-architecture/pack-format.md#sections) NAMES row:** the codec becomes
  "column table (records) + FSST heap with per-script tables". The section has three parts:
  1. A header with the FSST tables and the script → table map.
  2. A column table: `place_first` (per row + 1), then per record `lang` (an index into a
     pack-local language list), `script`, `type` and `text_offset` (per record + 1). The codec is
     chosen by `encode_auto`.
  3. The FSST text heap.
- **BLOBS row:** zstd frames with an optional dictionary, encoded by C zstd and decoded by `ruzstd`.
  This is confirmed.
- **Phase 1 step 1.4, B3/B4:** this note is the research. The implementation step measures the
  delta-coded `text_offset` on the hot path and the FSST gate in §5.
- **[Decisions log](../../03-decisions/log.md):** a decision for "FSST per script group for NAMES;
  zstd/`ruzstd` for BLOBS; no lz4" is to be recorded by the step that adopts this note.
- **Dependency policy:** `ruzstd` brings `unsafe` code into the reader through a dependency. Our
  crates stay `forbid(unsafe_code)`, but the [quality bar](../../06-engineering/quality-bar.md)
  should say whether audited dependency `unsafe` is acceptable behind the `zstd` feature. The
  alternative would be our own zstd decoder, which is a large job.

## 7. Risks and open items

- **Mixed-script data is not measured.** The Devanagari result rests on 176 names, about 6.7 KB.
  Re-measure on the consensus Nepal and India data, where Devanagari, and in India other Indic
  scripts, will be a large share. Also measure whether a combined Indic table beats one table per
  script.
- **Timings are indicative.** They come from one shared machine under load and from `Instant`
  loops. The implementation step re-measures with `criterion`, and in WASM, where the 8-byte-store
  decoder's bounds checks may cost more (**unverified**).
- **FSST trainer determinism:** checked once on one corpus. Reproducible builds need it to hold
  always. Gate it in CI, or own the trainer.
- **`ruzstd` speed and `unsafe`:** it decodes small dictionary frames about 3× slower than C. That
  is fine for BLOBS. If a cold NAMES region is ever added, measure again.
- **The real name model will change the sizes:** language and type columns, OSM `name:xx` tags and
  per-place deduplication against search keys.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| fsst-rs | 0.7.0 (2026-10-27) | 2026-11-07 | crates.io API |
| fsst (Lance) | 14.0.2 (2026-10-29) | 2026-11-07 | crates.io API |
| vortex-fsst | 0.93.0 (2026-11-06) | 2026-11-07 | crates.io API |
| ruzstd | 0.9.1 (2026-10-11) | 2026-11-07 | crates.io API, source |
| zstd | 0.14.3 (2026-10-16) | 2026-11-07 | crates.io API |
| zstd-sys | 2.1.2+zstd.1.5.7 | 2026-11-07 | crates.io API |
| lz4_flex | 0.14.0 (2026-07-14) | 2026-11-07 | crates.io API, source |
| lz4 (C bindings) | 1.28.1 (2025-01-07) | 2026-11-07 | crates.io API |
| rustc | 1.99.1 | 2026-11-07 | local toolchain |

## 9. Sources

- [FSST: Fast Random Access String Compression](https://www.vldb.org/pvldb/vol13/p2649-boncz.pdf),
  Boncz, Neumann, Leis, PVLDB 13(11), 2020.
- [Zstandard Compression and the application/zstd Media Type (RFC 8878)](https://www.rfc-editor.org/rfc/rfc8878),
  IETF, 2021. Frame header, dictionary ID and magic number.
- [fsst-rs](https://github.com/spiraldb/fsst), SpiralDB. Source of 0.7.0 read on 2026-11-07.
- [ruzstd](https://github.com/KillingSpark/zstd-rs). Source of 0.9.1 read on 2026-11-07.
- [zstd-rs](https://github.com/gyscos/zstd-rs). Bindings to libzstd 1.5.7.
- [lz4_flex](https://github.com/PSeitz/lz4_flex). Source of 0.14.0 read on 2026-11-07.
- [GeoNames NP export](https://download.geonames.org/export/dump/), 2026-11-06, as pinned in S1.
- Benchmark: a throwaway crate outside the repository (scratch `names-bench`). It is not kept. To
  reproduce, follow the method in §3.2: Hilbert-ordered places, names deduplicated per place,
  300,000 random places, and the S1 corpus.
