---
title: "Presence of optional fields: rank directories and sparse rows (B6)"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# Presence of optional fields: rank directories and sparse rows (B6)

## 1. Question

Optional per-place data (elevation, population, timezone, external IDs and the like) exists
for some rows only. Which rows have a component, and where is a row's value among the present
ones? The answer must cost little for absent data, read in a few nanoseconds, and hold from
10⁵ to 10⁷ rows. It feeds step 1.4's B6 and the COMPONENTS section (kind 12).

## 2. Summary and recommendation

- **Three presence forms, the smallest picked per component by the writer (D-076):** none when
  every row has it; a bitmap with a poppy-style rank directory when many rows do; the sorted
  present rows, a packed column, when few do. Measured (B6, 2026-11-07), the crossover is near
  10% of rows: at 0.1% the sorted rows are 70× smaller than the bitmap, at 50–99% the bitmap is
  1.6–3.6× smaller than the sorted rows.
- **Fields are dense:** one value per present row, so a row's value is at its rank.
- **Measured reads at 10⁶ rows** (load 11–13, indicative): rank 6.9 ns; presence through a
  bitmap 4.7 ns, through sorted rows 130 ns (mostly misses); a field read 7–20 ns with a
  bitmap or every row present.
- **Select is not built:** no reader needs "the k-th present row" yet (YAGNI); the sorted-rows
  form answers it in O(1) where it matters, the sparse case.

## 3. Findings

### 3.1 Rank directories

The format note's survey ([§3.3](format-compression-indexing.md#33-succinct-structures-rankselect-eliasfano))
covers rank9 (Vigna 2008, 25% overhead, one cache line per 512 bits) and poppy (Zhou, Andersen
and Kaminsky 2013, 3.125%: one u64 per 2048 bits holding the cumulative count and three 10-bit
sub-block counts). Poppy meets the target (rank ≤ 5 ns at an idle machine, overhead ≤ 6%) at an
eighth of rank9's space, so `teigeodata_core::bits` uses it. Its rank reads one entry, adds at
most three counts and popcounts at most eight words.

### 3.2 Sparse sets

A bitmap costs 1.03 bits per row whatever the density. A sorted list of the present rows costs
about log₂(gap) bits per present row. Elias–Fano costs 2 + log₂(n / m) bits per present row
(m present): at 1% that is 8.6 bits, against 11.4 for our packed column, so Elias–Fano would be
about 25% smaller in the sparse case. The packed column reuses the codec and the sorted search
already built for IDMAP (D-075), and the whole sparse component is tiny (0.014 B per row at 1%),
so the 25% is not worth a second structure.

### 3.3 Real optional data clusters

A source that covers one region (a national census, a DEM tile set) gives runs of present rows
in Hilbert order. Runs (mean length 64) make the sorted rows 10–30% bigger than scattered rows
at the same density and leave the bitmap unchanged, which moves the crossover a little lower;
the writer measures both forms per component, so it needs no threshold.

## 4. Options compared

| Option | B per row at 1% / 50% / 99% (10⁷ rows, random) | Presence read | Verdict |
|---|---|---|---|
| Bitmap + poppy rank | 0.129 / 0.129 / 0.129 | 4.7 ns | **Dense form** |
| Sorted rows, packed column | 0.014 / 0.325 / 0.209 | 130 ns | **Sparse form** |
| Elias–Fano | ~0.011 / ~0.19 / ~0.25 | ~20 ns (select) | Rejected: a second structure for 25% of a tiny column |
| rank9 | 0.156 / 0.156 / 0.156 | ~3 ns | Rejected: 8× poppy's directory for a nanosecond or two |
| Roaring (containers per 2¹⁶ rows) | adaptive | ~10–20 ns | Rejected: our two forms cover its cases with less code |

## 5. What we build ourselves, and why

- `teigeodata_core::bits`: the bitmap and its directory, a zero-copy view, O(1) parse and an
  O(len) check of the directory. Column-table codec 1.
- `teigeodata_format::components`: the section, its writer choosing the smallest form, and a
  `Field` handle that keeps a validated column so a read is O(1).
- The sorted-rows search is the shared `sorted.rs` of IDMAP and REDIRECTS, with the high column
  optional.

## 6. Impact on the plan

- Phase 1 step 1.4, B6: done as above; results in the Phase 1 benchmarks page.
- [Pack format](../../04-architecture/pack-format.md): codec 1 and the COMPONENTS section.
- The forge (Phase 3) assigns component and field numbers from the schema's components.

## 7. Risks and open items

- Sparse lookups take 130 ns. If a workload reads a sparse component for every result, a
  per-block hybrid (a bitmap for dense 2048-row blocks, rows for sparse ones) is the upgrade.
- The timings were taken at a load of 11–13; rerun on an idle machine before quoting them as
  final.

## 8. Versions checked

No new dependencies.

## 9. Sources

- [Space-Efficient, High-Performance Rank and Select Structures on Uncompressed Bit Sequences](https://doi.org/10.1007/978-3-642-38527-8_15), Zhou, Andersen and Kaminsky, SEA 2013.
- [Broadword Implementation of Rank/Select Queries](https://doi.org/10.1007/978-3-540-68552-4_12), Vigna, WEA 2008.
- [Quasi-succinct indices](https://doi.org/10.1145/2433396.2433409), Vigna, WSDM 2013 (Elias–Fano in practice).
- [Better bitmap performance with Roaring bitmaps](https://doi.org/10.1002/spe.2325), Chambi, Lemire, Kaser and Godin, 2016.
- The [format note §3.3](format-compression-indexing.md#33-succinct-structures-rankselect-eliasfano), 2026-11-06.
