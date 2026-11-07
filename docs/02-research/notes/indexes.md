---
title: "Indexes: spatial, region and search (Phase 2)"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# Indexes: spatial, region and search (Phase 2)

## 1. Question

Phase 2 builds the three indexes of milestone M2 ([roadmap](../../07-planning/roadmap.md)):

- **PLACE_INDEX** (B8): nearest places and places in a box;
- **REGION** and **POLY_INDEX** (B9): which zones and admin units hold a point, exactly;
- **SEARCH** (B7): names to places, with the phonetic key.

This note fixes what each is built from, what is measured against what, and the gates. It builds
on the findings already made, and does not repeat them:

- [format, compression and indexing](format-compression-indexing.md) §3.5 and §3.6: the options
  and crates;
- [spike S2](../benches/s2-region-index.md): the exact region method, measured on the world
  timezones;
- [spike S3](../benches/s3-search.md) and [names and search](names-and-search.md): the search
  design (D-052).

## 2. Summary and recommendation

1. **PLACE_INDEX is a packed Hilbert R-tree over the PLACES rows as they are.** The rows are
   already in Hilbert order (D-071, D-074), so the leaves are the rows: the index stores only
   node boxes, and reads coordinates from PLACES. Node size 16 gives about n/15 boxes. With i32
   boxes that is about 1.1 B per place; boxes relative to the parent in u16 halve it (A/B). A
   KDBush layout would need a row permutation (4 B per place, more than the whole tree) and is
   rejected without a build. Queries: box (stack descent) and kNN (best-first on a bounded
   heap, an equirectangular lower bound on boxes, exact distance on rows), with the
   antimeridian handled by splitting the query. Oracle: brute force over every row, and
   `geo-index` 0.4.0 on the same points.
2. **REGION is the S2 spike's method in a packed layout**: a dense top grid, Morton-sorted
   leaves, each either full (a zone set) or a border leaf (the parity at its reference point and
   runs of edge IDs), edges stored as shared arcs (each border once), delta-coded in B1 blocks.
   Exact `i64`/`i128` integer predicates, the spike's three rules, and a documented tie-break for
   points on an edge. Gates: zero mismatches against the even–odd reference on the world
   timezones with oceans (uniform, 1 cm from edges, real places); below 100 ns per query on
   uniform world points and on Nepal places; size against the spike's 43 MB estimate. Rival:
   `tzf-rs` 2.3.0 with its full data.
3. **SEARCH keeps the S3 design** (three FSTs, our own anchored Damerau automaton, one additive
   score) and moves it into the section with the measured problem fixed: p99 78–116 µs is the
   automaton, so Phase 2 measures a Schulz–Mihov parametric automaton (`levenshtein_automata`
   0.2.1 as the reference) and top-k by importance-ordered postings with an early stop, against
   S3's numbers on the 512-query golden set. Recall gates stay those of D-052.
4. **ID maps are done:** M2's "perfect-hash ID maps" was answered by B5 (D-075: a sorted list,
   12× smaller, PtrHash kept as the upgrade path). The roadmap wording is updated.

Order: B8 first (smallest, unblocks reverse geocoding), then B9 (largest data work), then B7.

## 3. Findings

### 3.1 PLACE_INDEX over the existing row order

- A packed Hilbert R-tree (Kamel and Faloutsos 1994; flatbush) is a bulk-loaded tree whose
  leaves are the items in Hilbert order and whose levels are stored contiguously, node size `M`.
  Level sizes are ⌈n/M⌉, ⌈n/M²⌉, …: about n/(M−1) boxes in all.
- Our PLACES rows are sorted by the Hilbert index of their 1e-7° grid point, the same order a
  flatbush build produces. So the leaf level, which flatbush stores as one box per item, is
  implied by the coordinates already in PLACES (B1 columns, 3–10 ns per read, D-074). This is
  where the index saves most: flatbush stores 16 B (i32) or 32 B (f64) per item at the leaves.
- Box size per node level, at n = 10⁷ places and M = 16: 666,667 boxes; at 16 B each 10.7 MB
  (1.07 B per place); with boxes as u16 offsets inside their parent 5.3 MB. The column table
  makes either a set of B1 columns (min lat, min lon, max lat, max lon per level), so the A/B
  is a codec choice inside one structure.
- **kNN** is best-first search (Hjaltason and Samet 1999) on min-distance to a node box: optimal
  in node visits. A bounded binary heap of node and row candidates on a fixed-size buffer keeps
  it allocation-free; `k` is capped by the API.
- **Distance.** The lower bound on a box must never exceed the true distance. On the sphere an
  equirectangular bound with the cosine of the latitude nearest the equator inside the box is
  expected to be a lower bound (**unverified**: a property test against haversine decides; the
  fallback is the exact great-circle distance from a point to a latitude–longitude box, which
  costs a few trigonometric calls per node); final ranking uses haversine on the rows. Both run on integer grid values
  converted once per query.
- **Antimeridian.** A box query crossing ±180° is split into two; kNN computes longitude
  differences modulo 360°.
- **KDBush** (a static k-d tree, the other A/B in the format note) sorts items by alternating
  axes, a different order from Hilbert, so it needs a row permutation: 4 B per place, more than
  the whole R-tree. Rejected on size; no build needed.

### 3.2 REGION

The method is proven exact (S2: zero mismatches in the domain on 20,000 uniform points and
20,000 points 1 cm from edges). What Phase 2 adds:

- **Layout.** The spike's pointer tree costs cache misses on ocean points (190 ns uniform
  against 44 ns on places). A dense top grid (one directory read) plus Morton-sorted leaves
  bounds a query to a directory read, a short descent and one leaf.
- **Shared arcs.** Neighbouring zones share their border; storing each border once is
  estimated to halve the 33 MB of vertices (S2 conclusion 2). The plan was i32 deltas in B1
  blocks with a ramp-coded offsets column; B9 measured absolute FOR-coded x and y smaller, and
  edges are addressed by vertex number with no offsets column (D-083, 20.5 MB for 4.1 M
  vertices).
- **Precedence** for points in more than one zone is open question Q-013; the index returns
  the set, and precedence is applied above it.

### 3.3 SEARCH

- S3 met recall@1 0.953 and recall@5 0.986 on Nepal at 3.6 MB; its p99 of 78–116 µs is spent in
  the fuzzy automaton walk (D-052).
- A Schulz–Mihov parametric automaton precomputes the state transitions for a distance bound,
  so the per-character cost is a table lookup; `levenshtein_automata` 0.2.1 implements it for
  `fst`-style automata, Damerau included (**unverified** for transpositions with our folding;
  the A/B checks).
- `fst` 0.4.7 (2021) is unmaintained but stable and in place; its on-disk format is stable.
  It needs `std` only for building.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Version | Verdict |
|---|---|---|---|---|---|
| Hilbert R-tree over PLACES rows, i32 boxes | No leaf level, 1.1 B per place, in place | Our code | ours | n/a | **Build** |
| Same, u16 boxes inside the parent | Half the size | A decode per box | ours | n/a | A/B |
| flatbush ABI, f64 (`geo-index`) | Ecosystem reads it | 32 B per item at the leaves | MIT/Apache | 0.4.0 | Oracle |
| KDBush layout | Simple | Needs a 4 B permutation | ISC | n/a | Reject (size) |
| `rstar`, `kiddo` | Mature | Own memory, build at load | MIT/Apache | 0.14.0 / 6.3.0 | Oracles |
| Packed S2-style region index | Exact, proven in S2 | Builder work | ours | n/a | **Build** |
| `tzf-rs` full data | ~100–160 ns | Not exact with lite data | MIT, data ODbL | 2.3.0 | Rival |
| Own anchored Damerau automaton (S3) | Built, recall met | p99 up to 116 µs | ours | n/a | Baseline |
| Parametric automaton | Table lookups | Fit to our folding unverified | MIT | `levenshtein_automata` 0.2.1 | A/B |

## 5. What we build ourselves, and why

Each index is our own reader on our own layout, because each must be read in place from a
section with no allocation, `no_std`, and none of the crates offers that on our row order and
grid. Each has a brute-force oracle in its tests and a fuzz harness on its section bytes, as
every Phase 1 section has.

## 6. Impact on the plan

- [Phase 2](../../07-planning/phases/phase-2.md) is written from this note: B8, then B9, then B7.
- The roadmap's M2 drops "perfect-hash ID maps" (done in B5, D-075).
- [pack-format](../../04-architecture/pack-format.md) PLACE_INDEX: the row order of PLACES is
  the leaf level; only node boxes are stored.

## 7. Risks and open items

- The equirectangular lower bound near the poles: proven by a property test against haversine
  on random boxes, including boxes touching ±90°.
- Arc sharing needs topologically clean sources; timezone-boundary-builder borders may not
  share vertices exactly. B9 measures how much is shared.
- Q-013 (zone precedence) must be answered before REGION returns a single zone.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| `geo-index` | 0.4.0 (2026-09-15) | 2026-11-07 | crates.io |
| `static_aabb2d_index` | 2.1.0 (2026-08-08) | 2026-11-07 | crates.io |
| `rstar` | 0.14.0 (2026-11-01) | 2026-11-07 | crates.io |
| `kiddo` | 6.3.0 (2026-09-06) | 2026-11-07 | crates.io |
| `tzf-rs` | 2.3.0 (2026-11-02) | 2026-11-07 | crates.io |
| `fst` | 0.4.7 (2021-06-06) | 2026-11-07 | crates.io |
| `levenshtein_automata` | 0.2.1 (2021-05-18) | 2026-11-07 | crates.io |

## 9. Sources

- Kamel and Faloutsos, *Hilbert R-tree: An improved R-tree using fractals*, VLDB 1994.
- Hjaltason and Samet, *Distance browsing in spatial databases*, ACM TODS 24(2), 1999.
- Schulz and Mihov, *Fast string correction with Levenshtein automata*, IJDAR 5, 2002.
- [flatbush](https://github.com/mourner/flatbush), the packed Hilbert R-tree layout, accessed 2026-11-07.
- The format note, spikes S2 and S3, and D-052, D-071, D-074, D-075, linked above.
