---
title: "Integrity for large sections: lazy per-block verification (B12)"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# Integrity for large sections: lazy per-block verification (B12)

## 1. Question

A pack is opened in 0.7 µs because `Pack::open` reads only the head. Hashing each section whole
costs 55.5 ms per 100 MB ([phase-1 benches](../benches/phase-1.md#container-l1-to-l3), L2). A DEM
or geometry section of 100 MB to several GB, memory-mapped, cannot be hashed whole on first use
without a visible stall. How should such a section be verified lazily, block by block on first
read, and what goes in section 15, BLOCK_HASHES
([pack format](../../04-architecture/pack-format.md#sections))? This feeds Phase 1 step 1.4, item
B12, and the default of `Pack::verify_sections` versus `Section::verify`
([verification on open](../../04-architecture/pack-format.md#verification-on-open)).

## 2. Summary and recommendation

Store, for every section of **1 MiB or more**, the **BLAKE3 chaining value (CV) of each 16 KiB
block**: a flat list of 32-byte values, exactly the leaves of the section's own BLAKE3 tree at
16-chunk granularity. Merging the list in BLAKE3 tree order gives the section hash already in the
section table, so:
- the list needs no trust root of its own: a reader proves a section's slice of the list by
  merging it (74 ns per block, 4.8 ms per GiB, once) and comparing with the table hash;
- the writer computes the CVs and the section hash in one pass;
- a full verify still runs at the speed of plain hashing, and parallelises across threads.

A first read of a random 4 KiB record then costs one 16 KiB hash: **9.7 µs median, 12 µs mean**
with NEON, against 680 ms to hash a 1 GiB section whole. The list costs 2 MiB per GiB (0.2 %).
A caller-supplied bitmap of atomics remembers verified blocks (0.7 ns per check), with no
allocation and no `unsafe` in the reader. No Bao crate is needed: `blake3::hazmat` (stable since
1.8.0) provides the two operations used. The exact layout is in [section 5](#5-what-we-build-ourselves-and-why).

**Unverified:** phone and wasm numbers (measured here only on this Mac, under load); cold reads
from flash (all numbers are memory-resident); that our 16 KiB leaves equal `bao-tree`'s chunk-group
leaves at its block size 4.

## 3. Findings

### 3.1 How hashing works now

- `crates/teigeodata-format` is `#![no_std]`, with `unsafe_code = "forbid"` from the workspace
  lints. It depends on `blake3 = { version = "1.8.7", default-features = false }`.
- Each section table entry holds `blake3[32]`: `blake3::hash` of the stored bytes.
  `Section::verify` recomputes it; `Pack::verify_sections` does so for every section; the head
  digest covers the table, so it covers every section hash.
- BLOCK_HASHES (kind 15) is reserved in `Kind`, with no layout yet. The earlier survey
  ([format, compression, indexing](format-compression-indexing.md)) proposed a 64 KiB list for
  sections over 1 MiB, with Bao "an option to evaluate, not a v1 dependency".

### 3.2 BLAKE3's tree and `hazmat`

- BLAKE3 is a binary Merkle tree over 1 KiB chunks. The left subtree of any node holds the largest
  power of two of chunks smaller than the total, so a run of equal power-of-two blocks (the last
  one may be short) maps exactly onto subtrees of the tree.
- `blake3::hazmat` (added in 1.8.0, 2025-03-31, replacing the undocumented `guts`) gives
  `HasherExt::set_input_offset` and `finalize_non_root` (the CV of a subtree at an offset) and
  `merge_subtrees_non_root` / `merge_subtrees_root` (join two CVs). Its docs warn it is "hazardous
  material" and ask users to test against `blake3::hash`; the bench does that on odd lengths
  (1 MiB + 1, 3 × 64 KiB, 320 KiB + 777, 7 MiB, 128 KiB + 1) at every block size, and on the full
  256 MiB buffer. All matched.
- Features of `blake3` 1.8.7: `default = ["std"]`, `pure`, `neon`, `no_neon`, `wasm32_simd`,
  `prefer_intrinsics`, `rayon`, `mmap`, `zeroize`, `digest`, and the `no_*` x86 switches. On
  little-endian aarch64 the NEON code is on by default unless `pure` or `no_neon` is set
  (`build.rs`), which is why the `neon` feature changed nothing in Phase 1. That NEON code is C,
  built with `cc`; `pure` is Rust only. Our `forbid(unsafe_code)` binds our crates only: `blake3`
  uses `unsafe` internally under every feature set.

### 3.3 Measurements (2026-11-07)

Throwaway crate, `blake3` 1.8.7, Rust release build, Apple Silicon Mac (aarch64), one thread
unless stated, a 256 MiB pseudo-random buffer held in memory and reused. The machine was shared:
load average 11 to 12 during the runs, so absolute numbers are 15 to 20 % below the Phase 1
idle figure (1.76 GiB/s). Two runs of the NEON build agreed within that margin; the table gives
the first.

Whole-section hash (option 1): **1.47 GiB/s NEON** (second run 1.36), **0.51 GiB/s `pure`**.

| Block | Flat hash GiB/s (NEON / pure) | CV GiB/s (NEON / pure) | Random 4 KiB first read, p50 / p99 / mean (NEON) | Same, `pure` | Bao path check (NEON / pure) | List per GiB | Bao outboard per GiB |
|---|---|---|---|---|---|---|---|
| 4 KiB | 1.42 / 0.63 | 1.48 / 0.57 | 3.1 / 7.5 / 4.0 µs | 8.9 / 27.9 / 11.7 µs | 8.3 / 18.6 µs | 8 MiB | 16 MiB |
| 16 KiB | 1.43 / 0.56 | 1.48 / 0.61 | 9.7 / 19.5 / 12.0 µs | 20.4 / 248 / 48 µs | 10.9 / 30.1 µs | 2 MiB | 4 MiB |
| 64 KiB | 1.48 / 0.58 | 1.42 / 0.47 | 36.8 / 108 / 41.7 µs | 80.1 / 264 / 102 µs | 42.5 / 98.2 µs | 512 KiB | 1 MiB |
| 256 KiB | 1.33 / 0.58 | 1.48 / 0.56 | 144 / 347 / 160 µs | 324 / 1969 / 455 µs | 181 / 417 µs | 128 KiB | 256 KiB |

- **Random first read:** pick a random 4 KiB record, hash the block holding it, compare with the
  already-proven list entry. The 256 MiB buffer is far larger than the caches, so the block comes
  from DRAM, not from flash; a true cold read adds the device's I/O time.
- **Bao path check:** the same read with no proven list: hash the block, then `log2(n)` parent
  merges up to the root, as Bao does. Within 1 to 3 µs of the list check at 16 KiB.
- **Merging a list to its root:** 73 to 75 ns per block (one compression), so 4.9 ms per GiB at
  4 KiB, **1.2 ms per 256 MiB (4.8 ms per GiB) at 16 KiB**, 0.30 ms per 256 MiB at 64 KiB.
- **Block size does not change full-verify throughput** at 16 KiB and above: NEON keeps 1.4 to
  1.5 GiB/s per core whether one call hashes 256 MiB or 16 KiB.
- **Parallel full verify** (std threads, four of them, 64 KiB CVs): 4.97 GiB/s NEON, 1.95 GiB/s
  `pure`. Independent blocks let the engine use its own threads; the reader stays single-threaded
  and `no_std`.
- **NEON against `pure`:** 2.9 times faster on whole hashing (1.47 against 0.51 GiB/s). `pure`
  is what a wasm build gets without `wasm32_simd`.
- **Verified-bitmap check** (an `AtomicU64` load and a bit test): 0.67 ns NEON build, 0.84 ns
  `pure` build.
- Plain Bao (1 KiB chunks) needs 64 MiB of outboard per GiB (6.25 %).

### 3.4 Bao implementations

- `bao` 0.13.1 (2025-04-07), the reference: 2,648 lines of source, built on `std::io`, no `no_std`.
- `bao-tree` 0.17.0 (2026-10-22, n0, used by iroh-blobs): 10,173 lines; depends on
  `positioned-io`, `range-collections`, `smallvec`, `self_cell`, `bytes`, and by default on
  `tokio`-side crates; no `no_std` attribute. It supports chunk groups (16 KiB suggested).
- `abao` 0.2.0 (2023-02-27): stale.
- None fits a `no_std`, allocation-free reader. Both rely on the `blake3` tree operations that
  `hazmat` exposes, which is all we need.

### 3.5 mmap and partial downloads

- **mmap:** hashing a block on first touch faults in exactly that block. At 16 KiB that is one
  page on Apple platforms and on Android's 16 KB-page devices, and four pages on 4 KiB-page Linux,
  where readahead fetches more anyway. A verified block stays valid only while the file does not
  change: the install rule "a file that may be mapped is never written over"
  ([pack format](../../04-architecture/pack-format.md#physical-file-names-and-installs)) is what
  makes memoising safe. Truncation and SIGBUS remain B13's question.
- **Downloads:** sections lie in kind order, so BLOCK_HASHES (15) comes after DEM (13) in the
  file. The writer can therefore stream a large section, keep only its CVs, and write the list
  afterwards. A downloader fetches the head (one range request, 16 KiB), then the BLOCK_HASHES
  range, proves each section's slice against the table hash, then fetches large sections in
  ranges and **checks every 16 KiB block before writing it**. A bad mirror is caught within one
  block; a resumed download keeps every block already checked and re-fetches only a partial one.
  This gives the benefit [offline loading](offline-loading-and-trust.md) expected from a Bao
  outboard, with no separate asset and no extra dependency.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| (1) Eager whole-section hash (now) | Simplest; no extra bytes | 680 ms per GiB (NEON), 2 s per GiB (`pure`) before the first read; one bad byte rejects the whole section | CC0/Apache (`blake3`) | 1.8.7 | Keep for sections under 1 MiB |
| (2) Flat list of independent `blake3::hash` per block, covered by the BLOCK_HASHES table hash | Simple; same per-read cost as (3) | Writer hashes twice (blocks and whole) or the section hash stops meaning BLAKE3 of the bytes; list proven only by hashing the whole BLOCK_HASHES section; eager verify hashes again | as above | as above | Rejected: (3) costs the same and does more |
| (3a) Bao outboard (`bao-tree`, 16 KiB groups) | Verifies a range from the root alone; iroh interop | Twice the bytes (4 MiB per GiB, parents included); `std`, 10k lines, several dependencies; parents are redundant when the whole leaf list ships in the pack | MIT/Apache | 0.17.0 | Rejected for the reader |
| (3b) Flat list of BLAKE3 leaf CVs at 16 KiB, proven by merging to the table hash (ours, on `hazmat`) | Same size as (2); one writer pass; the table hash stays plain BLAKE3; per-section proof; `no_std`, no allocation, about 100 lines | Uses `hazmat`, which needs careful tests | as above | `hazmat` since 1.8.0 | **Use** |

Block size within (3b):

| Block | First read p50 NEON | p50 `pure` | List per GiB | Merge per GiB | Verdict |
|---|---|---|---|---|---|
| 4 KiB | 3.1 µs | 8.9 µs | 8 MiB (0.78 %) | 19 ms | Too much list |
| 16 KiB | 9.7 µs | 20.4 µs | 2 MiB (0.20 %) | 4.8 ms | **Use**: matches 16 KB pages and `bao-tree`'s default |
| 64 KiB | 36.8 µs | 80.1 µs | 512 KiB (0.05 %) | 1.2 ms | 4× the read latency to save 0.15 % |
| 256 KiB | 144 µs | 324 µs | 128 KiB | 0.3 ms | Too slow per read |

## 5. What we build ourselves, and why

**Layout of BLOCK_HASHES** (encoding 0 RAW, little-endian, no padding):

```
header (16 B):  block_log2 u8 (= 14, 16 KiB), reserved[3] (0), entry_count u32,
                cv_count u64
directory:      entry_count × 16 B { kind u32, reserved u32 (0), first u64 }
                kinds strictly increasing, each a section present in the table
cvs:            cv_count × 32 B, the leaf chaining values
```

- Entry `k` covers section `kind` with `count = ceil(length / 2^block_log2)` CVs starting at
  `cvs[first]`; `first` of entry 0 is 0, each next `first` is the previous `first + count`, and
  the last sum equals `cv_count`. The section length comes from the table, so it is not repeated.
- `cvs[i]` = `Hasher::new().set_input_offset(i << block_log2).update(block_i).finalize_non_root()`;
  the last block may be short.
- **Coverage rule:** every section of length ≥ 1 MiB (64 blocks or more) has an entry; no smaller
  section has one; BLOCK_HASHES itself never does. Below 1 MiB a whole hash costs at most 0.7 ms
  NEON, 2 ms `pure`. The writer refuses a list whose merge does not equal the section hash.
- BLOCK_HASHES has its own table hash like any section, so `verify_sections` and the head digest
  cover it unchanged.

**Reader, `no_std`, no allocation, no `unsafe`:**
1. `Section::blocks(&self, list: &BlockHashes) -> Result<Blocks<'a>>` finds the entry, checks the
   counts against the length, and merges the slice in BLAKE3 tree order (left subtree = largest
   power of two of blocks below the count) with `merge_subtrees_non_root`, finishing with
   `merge_subtrees_root`; it compares the result with the table hash. The recursion is at most 64
   levels deep with 32-byte values on the stack. Cost: 74 ns per block, once per section.
2. `Blocks::verify(i) -> Result<&'a [u8]>` hashes block `i` with a `Hasher` on the stack (about
   1.9 KB), at offset `i << 14`, and compares with `cvs[i]`. `Blocks::bytes(range)` verifies each
   block the range touches.
3. Memoising is optional and caller-owned: `Blocks::with_memo(&'b [AtomicUsize])` takes
   `ceil(count / usize::BITS)` words (8 KiB per GiB on 64-bit), which the engine allocates once
   per open pack. A check is an `Acquire` load and a bit test; a success sets the bit with
   `fetch_or(Release)`. Two threads that verify the same block at once both succeed; no lock is
   needed. Without a memo, every access re-hashes its blocks.
4. `Pack::verify_sections` keeps its meaning. For a covered section it may hash the blocks to CVs
   and merge them: the same single pass, and the engine may split the blocks across threads.

**Measurements that must prove it in the implementation step:** the `hazmat` merge equal to
`blake3::hash` on the official test-vector lengths and a property test over random lengths; a
golden pack; a fuzz case that flips one byte in each covered block and in the list and must be
rejected; the L2 bench extended with a lazy row (proof of the list, then one random record) for
100 MB and 1 GB packs; a deliberate wrong CV that the check must reject first (AGENTS.md §6).

## 6. Impact on the plan

- [Pack format](../../04-architecture/pack-format.md): fill the BLOCK_HASHES row (16 KiB leaf
  CVs, sections ≥ 1 MiB), add the layout above, and state in "Verification on open" that the
  default is lazy per block for covered sections and lazy whole-section for the rest.
- [Benchmarks](../../06-engineering/benchmarks.md) L2: the lazy case is defined here; add the
  first-read row.
- [Format, compression, indexing](format-compression-indexing.md) proposed 64 KiB independent
  hashes; this note replaces that with 16 KiB leaf CVs.
- [Offline loading and trust](offline-loading-and-trust.md): the downloader checks blocks with
  this list instead of a Bao outboard.
- A decision-log entry for B12 when the maintainer's agents adopt it.

## 7. Risks and open items

- `hazmat` is the crate's own sharp edge: a tree-order mistake gives wrong hashes that look
  valid. The tests in section 5 are the guard.
- Phone (Android arm64, iOS) and wasm (`pure`, `wasm32_simd`) numbers are **unverified**; the
  `pure` column is the floor estimate.
- Cold reads from flash were not measured (no root to drop the page cache); a device bench in
  Phase 4 with the mmap engine.
- That our 16 KiB leaves equal `bao-tree` leaves at its block size 4, which would let a future
  hosted tier serve standard Bao ranges from our list, is **unverified**.
- Code size of the reader addition was not measured; estimated at about 100 lines plus the
  `hazmat` merge functions already in `blake3`. Z2 measures library size.
- A future block size other than 16 KiB needs only a new `block_log2`; readers refuse values
  they do not know.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| blake3 | 1.8.7 (2026-08-20); `hazmat` since 1.8.0 (2025-03-31) | 2026-11-07 | crates.io API, GitHub releases |
| bao-tree | 0.17.0 (2026-10-22) | 2026-11-07 | crates.io API and source |
| bao | 0.13.1 (2025-04-07) | 2026-11-07 | crates.io API and source |
| abao | 0.2.0 (2023-02-27) | 2026-11-07 | crates.io API |

## 9. Sources

- [blake3 1.8.7 `hazmat` module](https://docs.rs/blake3/1.8.7/blake3/hazmat/index.html), BLAKE3
  team, accessed 2026-11-07; source and `build.rs` read from the 1.8.7 crate.
- [BLAKE3 releases](https://github.com/BLAKE3-team/BLAKE3/releases), 1.8.0 notes on `hazmat`,
  accessed 2026-11-07.
- [BLAKE3 specification](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf),
  section 2.1 (tree structure), O'Connor, Aumasson, Neves, Wilcox-O'Hearn, 2020.
- [Bao](https://github.com/oconnor663/bao) and [bao-tree](https://github.com/n0-computer/bao-tree),
  accessed 2026-11-07; crate sources 0.13.1 and 0.17.0 read for size and `no_std`.
- Throwaway bench (not kept): `blake3` 1.8.7 default and `pure`, 256 MiB buffer, run 2026-11-07.
