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
- [ ] Node boxes per level over the Hilbert-ordered rows, node size A/B (8, 16, 32); i32 boxes
  against u16 boxes inside the parent
- [ ] Box query and kNN (best-first, bounded heap, no allocation), antimeridian split; the
  distance lower bound proven by a property test
- [ ] Oracles: brute force over every row, and `geo-index` 0.4.0; fuzz harness on the section
- [ ] Measured on Nepal and on 10⁷ synthetic world points: size per place, box and kNN (k = 1,
  10) latency, against `geo-index` and `rstar`

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
