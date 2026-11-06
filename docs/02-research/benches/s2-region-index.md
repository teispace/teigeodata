---
title: "Spike S2: exact region index on the world timezone boundaries"
status: research
updated: 2026-11-06
---

# Spike S2: exact region index on the world timezone boundaries

**Input:** timezone-boundary-builder 2026f `timezones-with-oceans.geojson.zip` (SHA-256
`838b3c91…21ac9`): 444 zones, 8,296,476 vertices and 8,294,328 edges, quantised to the 1e-7°
grid ([ADR-0029](../../03-decisions/adr/ADR-0029-coordinate-grid.md)). GeoNames `NP.txt`
(148,210 places) is used for real-place timings.

**Code:** `spikes/s2-region-index`. The run is
`LEAF_MAX=<n> cargo run --release -p spike-s2-region-index -- <combined-with-oceans.json> <NP.txt>`.

**Machine:** Apple silicon laptop (shared, load 3–5), single-thread timings. The numbers are
indicative; Phase 2 re-measures under [the benchmark method](../../06-engineering/benchmarks.md)
(Q3).

## Design measured

- A quadtree over the 1e-7° grid. Each node has a reference point strictly inside its cell that
  never lies exactly on one of its edges.
- Zone parity at a child's reference point is derived from the parent's by counting crossings
  along the segment between the two points.
- A leaf keeps the original source edges that touch it, the parity of their zones at its
  reference point, and the set of zones covering it without edges.
- A query counts crossings from the leaf's reference point to the query point. All predicates
  are exact `i128` integer arithmetic.
- The reference is an independent even–odd ray cast to the east over every edge.

## Results

### Exactness (20,000 points each; result sets compared, not just the first zone)

| Set | On an edge (excluded) | In more than one zone | Mismatches |
|---|---:|---:|---:|
| Uniform world | 0 | 56 | **0** |
| 1 grid unit (about 1 cm) from a random edge | 2,303 | 294 | **2**, both outside the coordinate domain (lat 90.0000001°, lon −180.0000001°) |

The same results held at every leaf size tested (8, 16, 32, 64 and 128).

The first run had 56 and 1,214 mismatches. These came from three bugs:

1. A cell could store only one "background" zone, so overlapping zones were lost.
2. Reference points could leave cells at depth 28, where cells are about 13 units wide and the
   fixed offset was +7/+13.
3. Reference points could fall exactly on an edge, which makes that edge's crossing invisible to
   both segments that end there.

### The source zones overlap

The boundaries are not a partition. Points inside more than one zone:

| Overlap | Uniform | Near-edge |
|---|---:|---:|
| Asia/Shanghai + Asia/Urumqi | 54 | 222 |
| Asia/Hebron + Asia/Jerusalem | 0 | 24 |
| **Asia/Kathmandu + Asia/Kolkata** | 0 | **9** |
| Africa/Juba + Africa/Khartoum | 2 | 5 |
| Asia/Tbilisi + Europe/Moscow | 0 | 3 |
| Asia/Shanghai + Asia/Thimphu | 0 | 2 |
| America/Sitka + America/Vancouver; Rio_Gallegos + Punta_Arenas | 0 | 1 each |

Shanghai/Urumqi is deliberate in the source, because both are in use in Xinjiang. The others
are disputed or sliver areas. Two consequences:

- The index returns **every** zone holding the point.
- A single-zone answer needs a defined, documented precedence. For the Nepal–India border it
  must follow our perspective
  ([ADR-0006](../../03-decisions/adr/ADR-0006-perspective.md)) and not the order of the input.

### Size and speed against leaf size

The "packed v2" estimate assumes:

- edge-free leaves cost 10 B (a Morton key and a zone);
- other leaves cost 14 B;
- edge references are stored as runs of consecutive edge IDs at 5 B per run;
- vertices are delta-coded at about 4 B each.

| Edges per leaf (max) | Leaves (edge-free) | Edge refs | Refs per run | Packed v2 estimate | Uniform world ns | Nepal places ns |
|---:|---:|---:|---:|---:|---:|---:|
| 8 | 5,208,778 (2,764,622) | 13.4 M | 2.7 | 120.3 MB | 248 | 43 |
| 16 | 2,318,620 (1,185,594) | 10.7 M | — | — | 227 | 46 |
| 32 | 1,101,637 (546,954) | 9.6 M | 7.7 | 52.7 MB | 190 | 44 |
| 64 | 536,131 (259,576) | 9.0 M | 13.6 | **43.0 MB** | 197 | 46 |
| 128 | 265,072 (124,953) | 8.7 M | 24.1 | 38.2 MB | 229 | 64 |

The spike stores edges as structs and leaves as heap vectors. The naive u32 references cost
159.7 MB at 8 edges per leaf.

**Rival:** `tzf-rs` 2.3.0 with its default simplified data took 96–124 ns per query on uniform
world points and 74–81 ns on Nepal places. It agreed with our exact answer on 99.9969 % of
uniform points and 99.9865 % of Nepal places. Its errors are on borders, the case that matters
for places.

**Build:** 3.3–6.4 s single-threaded; peak RSS about 800 MB (spike data structures, not a
budget).

## Conclusions

1. **The method is exact.** It made no mismatches in the domain, including points 1 cm from
   borders. This confirms [ADR-0013](../../03-decisions/adr/ADR-0013-region-index.md). Three
   rules are now part of the design:
   - reference points stay strictly inside their cell and never lie on an edge;
   - covering zones are a set;
   - queries outside the domain are rejected at the API.
2. **Leaves of about 64 edges and edge-ID runs** give an estimated 43 MB for the full-resolution
   world with oceans. Vertices are the floor, at 33 MB. Shared arcs between neighbouring zones
   (each border stored once) should roughly halve that. Format benchmark B9/B10 measures it.
3. **Speed:** on real places we are already faster than the simplified rival (44 against
   74 ns). On uniform world points we are slower (190 against 110 ns). Uniform points are mostly
   ocean, so the cost there is cache misses in a deep pointer tree. A packed layout should fix
   that: a flat top-level grid, Morton-sorted leaves and edges stored next to their leaf. The
   target is below 100 ns on both.
4. **Points exactly on a border** need a documented rule: a half-open rule plus the zone
   precedence. Today the reference and the index each apply their own half-open rule.

## Next

- Phase 2 builds the packed layout and re-measures under Q3, including `tzf-rs` with full data.
- The zone-precedence rule for overlaps is open question Q-013.
