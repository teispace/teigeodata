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

### 2.2 B9 REGION
- [x] Packed layout: dense top grid with TOP_ZONES, Morton-sorted leaves, edge-ID runs; shared
  arcs as FOR-coded x and y (D-083, [layout](../../04-architecture/pack-format.md#region))
- [x] Exact: zero mismatches against the even–odd reference on the world timezones with oceans
  (20,000 uniform, 19,998 at 1e-7° from an edge, 148,210 Nepal places); the on-edge rule is the
  (ε, ε²) perturbation
- [x] Below 100 ns per query: 50 ns uniform and 41 ns on Nepal places at the default, against
  111 and 78 ns for `tzf-rs` 2.3.0 full data; 25.7 MB against S2's 43 MB estimate
  ([results](../../02-research/benches/phase-2.md#b9-region))
- [x] Q-013 (zone precedence) answered: ID order, set by the forge (D-084)
- POLY_INDEX moves to B10 with POLYGONS, whose boxes it indexes (D-083)

### 2.3 B7 SEARCH
- [x] The S3 design in one SEARCH section: one automaton over tagged keys (exact, loose,
  phonetic), FOR row lists by importance, per-row importance and block maxima (D-085,
  [layout](../../04-architecture/pack-format.md#search)); 1.50 MB on Nepal against S3's 3.63 MB
- [x] The automaton A/B: `fst` + byte row (S3), sorted keys + character row, sorted keys + NFA,
  own FST + NFA: 22.3 → 3.7 µs p50 per one-edit walk; the parametric arm was not needed
  ([results](../../02-research/benches/phase-2.md#b7-search))
- [x] Top-k prefix with an early stop on the block maxima (best 16 by importance); p99 41.2 µs
  unanchored against S3's 71.2 µs, recall gates of D-052 held, first-letter probe 1.000 and
  native one-letter probe 0.976 (D-086)
- [x] Oracles: the OSA dynamic program (200,000 cases), brute force over the keys and the
  section's input, exact refusal strings, a fuzz harness on the section; the counts gate covers
  `search_get`, `search_fuzzy` and `search_prefix`

### 2.4 Exit review
- [ ] M2 criteria met; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | Phase 2 planned; order B8, B9, B7 | the indexes note (D-081) |
| 2026-11-07 | B8 done: u16 boxes inside the parent, not i32 | measured 0.55 B against about 1.1 B per place (D-082) |
| 2026-11-07 | B9 done: absolute FOR vertices, not deltas with an offsets column; POLY_INDEX moved to B10 | measured smaller; POLY_INDEX indexes POLYGONS boxes (D-083) |
| 2026-11-07 | B7 done: own FST and a bit-parallel NFA, not `fst` and `levenshtein_automata`; unanchored fuzzy by default | `fst` is `std`-only and unsafe on hostile bytes; measured 6× faster and 2.4× smaller (D-085, D-086) |
