---
title: "Phase 2: indexes"
status: living
updated: 2026-11-07
---

# Phase 2: indexes

**Goal:** the spatial, region and search indexes, each read in place from its section, exact
where exactness is claimed, and A/B-benchmarked. Exit: milestone **M2** ([roadmap](../roadmap.md)).

**Research:** [indexes](../../02-research/notes/indexes.md), building on the
[format note](../../02-research/notes/format-compression-indexing.md) §3.5–3.6 and spikes
[S2](../../02-research/benches/s2-region-index.md) and [S3](../../02-research/benches/s3-search.md).

Every section follows the Phase 1 pattern: a column table where it can, a canonical encoder, a
validating zero-copy reader, a brute-force oracle in its tests, a fuzz harness in
`teigeodata_format::fuzz`, a criterion A/B and an instruction count in `benches/counts.rs`.

## Steps

### 2.0 Research
- [x] [Indexes](../../02-research/notes/indexes.md): PLACE_INDEX over the PLACES row order,
  REGION in a packed layout, SEARCH on the S3 design; KDBush rejected on size (D-081)

### 2.1 B8 PLACE_INDEX
- [x] Node boxes per level over the Hilbert-ordered rows, node size A/B (8, 16, 32); u16 boxes
  inside the parent chosen over i32: 0.55 B per place at node 16, against 38.4 B for
  `geo-index`'s f64 boxes (D-082, [layout](../../04-architecture/pack-format.md#place_index))
- [x] Box query and kNN (best-first on a fixed heap with a depth-first fallback, no
  allocation), antimeridian split; the distance lower bound proven by a property test on
  200,000 cases
- [x] Oracles: brute force over every row (7 sizes × 4 node sizes, a 60,000-place pack),
  `geo-index` 0.4.0 and `rstar` 0.14.0 as rivals; fuzz harness on the section; the counts gate
  covers `place_within` and `place_nearest`
- [x] Measured on Nepal and on 10⁶ and 10⁷ synthetic world points: a city box in 1.2–1.8 µs,
  k = 1 in 3.5–17 µs (4–12× the in-memory rivals, which are 70× bigger and rank by planar
  degrees) ([results](../../02-research/benches/phase-2.md#b8-place_index))

### 2.2 B9 REGION and POLY_INDEX
- [ ] Packed layout: dense top grid, Morton-sorted leaves, edge-ID runs; shared arcs in B1 blocks
- [ ] Exact: zero mismatches against the even–odd reference on the world timezones with oceans
  (uniform, 1 cm from edges, real places); the on-edge tie-break documented
- [ ] Below 100 ns per query on uniform world points and on Nepal places; size against S2's
  43 MB estimate; `tzf-rs` 2.3.0 full data as the rival
- [ ] Q-013 (zone precedence) answered

### 2.3 B7 SEARCH
- [ ] The S3 design in the SEARCH section (three FSTs, postings, scores)
- [ ] The automaton A/B: our anchored Damerau automaton against a parametric one; top-k with an
  early stop on importance-ordered postings; p99 against S3 on the golden set, recall gates of
  D-052 held

### 2.4 Exit review
- [ ] M2 criteria met; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | Phase 2 planned; order B8, B9, B7 | the indexes note (D-081) |
| 2026-11-07 | B8 done: u16 boxes inside the parent, not i32 | measured 0.55 B against about 1.1 B per place (D-082) |
