---
title: "Spike S1: coordinate, elevation and name codecs on Nepal"
status: research
updated: 2026-11-06
---

# Spike S1: coordinate, elevation and name codecs on Nepal

**Input:** GeoNames `NP.zip` (export of 2026-11-06, SHA-256
`e14643d6bfda199049196e1e17cbbb4a166732b23ce644910a35da499c29b0fd`), 148,210 places, 210,866 names.
**Code:** `spikes/s1-codecs` (`cargo run --release -p spike-s1-codecs -- data/cache/geonames-np/NP.txt`).
**Machine:** Apple silicon laptop (shared, load ~5), release build, single thread. Timings are
indicative only; Phase 1 re-measures with `criterion`.

## Results

## Coordinates (lat+lon), bytes per place

| grid | order | raw i32 | delta+LEB128 | delta+bitpack128 | delta+LEB128+zstd19 | decode LEB128 ns/coord | decode bitpack ns/coord |
|---|---|---:|---:|---:|---:|---:|---:|
| 1e-7 | source | 8.00 | 6.23 | 6.26 | 4.87 | 2.72 | 5.26 |
| 1e-7 | morton | 8.00 | 5.78 | 5.27 | 4.09 | 2.49 | 4.60 |
| 1e-7 | hilbert | 8.00 | 5.73 | 4.88 | 3.99 | 2.52 | 4.27 |
| 1e-6 | source | 8.00 | 5.31 | 5.43 | 4.55 | 3.75 | 4.66 |
| 1e-6 | morton | 8.00 | 4.50 | 4.51 | 4.01 | 2.42 | 3.89 |
| 1e-6 | hilbert | 8.00 | 4.35 | 4.06 | 3.90 | 2.34 | 3.54 |

## Elevation (GeoNames DEM column, metres), bytes per place

| order | raw i16 | delta+LEB128 | delta+bitpack128 |
|---|---:|---:|---:|
| source | 2.00 | 1.65 | 1.51 |
| hilbert | 2.00 | 1.51 | 1.25 |

## Names with random access

Offsets are counted as 4 bytes per string for raw and FSST, and 4 per block for blocks.

| set | strings | raw bytes | raw+offsets | FSST (+table) | zstd-3 blocks of 64 | lz4 blocks of 64 | whole zstd-19 (no random access) | FSST ns/string | zstd ns/block | lz4 ns/block |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| primary | 148210 | 1500257 | 2093097 | 1387241 | 838879 | 1192447 | 455792 | 34.3 | 2286 | 323 |
| all names | 210866 | 2303535 | 3146999 | 2055396 | 1146910 | 1619539 | 606316 | 34.9 | 2697 | 338 |

Search keys: 130730 unique lowercase names, 1427055 raw bytes, 833894 bytes front-coded (58.4%).

## Findings

1. **Hilbert order wins** for every codec and both grids: 22% smaller than source order and 7%
   smaller than Morton for bit-packed 1e-7 coordinates. Rows in packs are stored in Hilbert order.
2. **Point positions at 1e-6° (11 cm) cost 4.06 bytes/place** bit-packed, against 4.88 at 1e-7°.
   Point uncertainty is metres to kilometres, so the point column uses the 1e-6° grid; geometry
   (boundaries, region index) keeps 1e-7° ([ADR-0029](../../03-decisions/adr/ADR-0029-coordinate-grid.md)).
3. **Bit-packing beats LEB128 on size but my scalar decoder is slower** (3.4 vs 2.3 ns/coordinate).
   This is the naive byte-at-a-time decoder; a FastLanes-style unrolled or SIMD decoder is
   expected to be several times faster. Phase 1 benchmark B1 decides.
4. **Elevation** costs 1.25 bytes/place bit-packed in Hilbert order (neighbours have similar
   heights), against 2 bytes raw.
5. **Names, random access:**

   | codec | size vs raw+offsets | cost to fetch one name |
   |---|---:|---|
   | zstd-3, blocks of 64 | 0.40 | ~2.4 µs (decode the block) |
   | lz4, blocks of 64 | 0.57 | ~0.33 µs |
   | FSST | 0.66 | ~33 ns |

   Proposal for Phase 1: **FSST for hot display names** (one name per result, no block cost),
   **zstd blocks for cold alternate names**, and lz4 blocks as the middle option. The zstd
   numbers use the C library; the pure-Rust `ruzstd` decoder needed for WASM is measured in B4.
6. **Search keys** front-code to 58% of raw; the FST will do better (shared suffixes too).
7. **Budget check:** the 148k GeoNames places cost about 0.6 MB (positions) + 0.19 MB
   (elevation) + 0.8 to 1.4 MB (names) before indexes. A Nepal places pack of a few MB is
   realistic.

## Follow-ups

- B1: FastLanes/SIMD bit-unpacking vs LEB128 vs StreamVByte, with random access inside a block.
- B3/B4: FSST with per-script tables (Devanagari names), zstd with a trained dictionary per
  block set, `ruzstd` vs C zstd.
- Repeat on the consensus dataset once the forge exists (GeoNames ordering and precision are not ours).
