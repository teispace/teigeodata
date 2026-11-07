---
title: "ID to row: IDMAP and REDIRECTS (B5)"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# ID to row: IDMAP and REDIRECTS (B5)

## 1. Question

A pack answers "which row is place `tgd:np/3kq`?" (an app stored the ID) and "which ID is row
`r`?" (every result carries its ID). A retired ID must answer with its survivor, or with
"retired" for a tombstone (ADR-0008). Which layout does that in the least space and time, at
10⁵ (a country) to 10⁷ (the planet) places? This feeds step 1.4's B5 and the IDMAP (kind 1)
and REDIRECTS (kind 16) sections.

## 2. Summary and recommendation

- **Rows are fixed in Hilbert order** (D-074), so any ID → row map must store, for each key,
  where its row is: about log₂ n bits per key, whatever finds the key. An MPHF only finds the
  key, so it adds its ~3 bits on top. Its advantage is speed.
- **Measured (B5, 2026-11-07):** PtrHash with a plain permutation and verification takes 3 ns
  at 10⁵ and 27 ns at 10⁷, at 4.37 B per place. Our sorted list with the new **ramp** coding
  (§5) takes 82–91 ns for a hit and 32–41 ns for a miss at any size, at **0.35 B per place**
  when serials follow row order: 12× smaller, 3.4× slower at world size.
- **Decision (D-075): the sorted list and the permutation, on the column table,** because an
  offline pack's size matters more than 60 ns on a rare lookup (an app resolving IDs it stored).
  The forge allocates serials in row order (with shuffled serials ours is 4.6–6.4 B per place and
  up to 237 ns, worse than PtrHash on both). PtrHash stays the upgrade path if a workload needs
  bulk ID lookups ([ideas](../../01-product/ideas.md)).

## 3. Findings

### 3.1 What the ID is

A u64: a 16-bit namespace (the allocating country) and a 48-bit serial (D-070). A pack's IDs
mostly share one namespace; a place that moves country keeps its ID, so a pack can hold a few
from others. The B1 codec is u32, so an ID column is two columns: the high 32 bits (namespace
and the serial's top 16 bits, almost constant: 0 bits per value in a block) and the low 32
bits. Serials are allocated densely by the forge, so sorted low halves have small gaps.

### 3.2 Options

- **(a) Sorted IDs and a permutation** (searched by binary, Eytzinger or interpolation search).
  The format note's option (a). Size: the sorted IDs (small, delta) plus log₂ n bits per row.
  Time: a search over block bases (n / 128 u32 values: 4.7 KB for Nepal, 375 KB for the
  planet), one block unpacked, one `get`.
- **(b) MPHF + permutation + verification.** PtrHash (Groot Koerkamp, SEA 2025) at ~3 bits per
  key, a few ns per query when it fits in cache. Then `perm[h(id)]` gives the row, and the row
  → ID column confirms the ID (an unknown ID must be rejected; packs merge). Size: (a)'s
  permutation plus ~3 bits, minus the sorted IDs.
- **(c) Retrieval (static function), e.g. ribbon / BuRR** (Dillinger et al. 2022): stores
  f(id) = row in ~(1 + ε) log₂ n bits, membership by the same verification. Smallest for random
  orders; a large, delicate construction.

### 3.3 Redirects

ADR-0008: a retired ID maps to its survivor, or to nothing for a tombstone. They are few per
release and grow slowly; a sorted (old → new) list fits, with a 0 target for a tombstone
(serial 0 is never allocated, D-070). Lookup order: IDMAP first, REDIRECTS on a miss, one hop
(the forge flattens chains, so a redirect always names a current ID).

## 4. Options compared

| Option | Size per place (n = 10⁷) | Lookup | Code | Verdict |
|---|---|---|---|---|
| (a) sorted + permutation | ~24 bits + sorted IDs (a few bits) | search of block bases + one block + one `get` | small, shares the column table | **Build, measure** |
| (b) PtrHash + permutation | ~27 bits | hash, pilot, remap, `get`, verify `get` | a deterministic builder, a reader | Measure `ptr_hash` as the speed reference |
| (c) Ribbon retrieval | ~26 bits | several hashes, verify `get` | large | Reject unless (a) and (b) both fail |

## 5. What we build ourselves, and why

IDMAP and REDIRECTS as column tables, with one shared sorted-ID search (`sorted.rs`). Three
things made it fast, each measured at n = 10⁶ with serials in row order:

| Step | Size per place | Hit | Miss |
|---|---:|---:|---:|
| Delta-coded sorted IDs and permutation, binary search over blocks | 1.10 B | 216 ns | 103 ns |
| **Ramp** coding (`base + j + packed`): dense sorted serials and the near-identity permutation pack to almost nothing, and read in O(1) | 0.35 B | | |
| Interpolation over block firsts before the binary search, and no unpacking of a constant high-half block | 0.35 B | 87 ns | 35 ns |

The ramp coding is a codec change (coding 2, D-075), so every column benefits: the writer picks
it whenever it is smaller than FOR.

## 6. Impact on the plan

- Phase 1 step 1.4, B5: IDMAP and REDIRECTS as above; results in the Phase 1 benchmarks page.
- [Pack format](../../04-architecture/pack-format.md): IDMAP and REDIRECTS column tables.
- The forge (Phase 3): allocate a country's first serials in Hilbert order; flatten redirect
  chains.

## 7. Risks and open items

- (a) is 3.4× slower than (b) at 10⁷ and about 30× at 10⁵, in absolute terms 60–80 ns. Accepted
  for the 12× size saving (D-075); (b) is built if a measured workload needs it.
- Interpolation search would beat binary search on dense serials, but fails badly on clustered
  ones; binary search over block bases is the safe default.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| ptr_hash | 2.1.3 (2026-10-21) | 2026-11-07 | crates.io API |
| ph | 0.11.0 (2026-02-12) | 2026-11-07 | crates.io API |

## 9. Sources

- [PtrHash: Minimal Perfect Hashing at RAM Throughput](https://doi.org/10.4230/LIPIcs.SEA.2025.21), Groot Koerkamp, SEA 2025.
- [Fast Succinct Retrieval and Approximate Membership using Ribbon](https://doi.org/10.4230/LIPIcs.SEA.2022.4), Dillinger, Hübschle-Schneider, Sanders, Walzer, SEA 2022.
- [Array layouts for comparison-based searching](https://doi.org/10.1145/3053370), Khuong and Morin, ACM JEA 2017 (Eytzinger and B-tree layouts).
- The [format note §3.4](format-compression-indexing.md#34-minimal-perfect-hashing-stable-u64-id--row), 2026-11-06.
