---
title: "The DEM section: tiles, prediction and coding of elevation at a bounded error"
status: research
updated: 2026-11-08
owner: maintainer's agents
revisit: 2027-05-07
---

# The DEM section: tiles, prediction and coding of elevation at a bounded error

## 1. Question

Section kind 13 (DEM) of the [pack format](../../04-architecture/pack-format.md) is listed as
"tile directory + coded tiles at ±1 m", candidate codec "MED prediction + entropy coder"
(benchmark B11). This note designs it:

- a reader gets the elevation at any point of a country offline, from the section read in
  place, on every platform (wasm and mobile included), with bounded memory and no allocation
  per query;
- the forge writes it deterministically;
- the error bound is chosen against the DEM's own error: Copernicus GLO-30 LE90 18.7 m inside
  Nepal, NASADEM 17.7 m (D-063, D-064), target P90 ≤ 50 m ([AGENTS.md](../../../AGENTS.md) §11).

It builds on [format, compression and indexing §3.8](format-compression-indexing.md#38-dem-raster-compression-at-a-1-m-bound)
(first look and the 256×256 / MED draft), [timezone and elevation](timezone-and-elevation.md)
part B (DEM choice, datum, bilinear sampling), the [elevation reference](elevation-reference.md)
and [S6](../benches/s6-elevation.md). It does not repeat them.

## 2. Summary and recommendation

**Measured 2026-11-07 in Python on four Copernicus GLO-30 tiles** (flat Terai N26 E085,
mid-hills N27 E085, Himalaya N27 E086 with Everest, Himalaya N28 E084 with Manaslu), each
3584×3584 samples, scripts in §3.1. Sizes are bits per 1″ sample; "Nepal" is 176.4 M samples
inside the country, 180.1 M in the 64×64 tiles that touch it (§3.6).

1. **Predictor: per-tile least squares, not MED.** MED (LOCO-I) is built for image edges;
   terrain is smooth and sloped. A 6-tap linear predictor (left, above, above-left,
   above-right, left-left, above-above) with integer coefficients fitted per tile by the forge
   (5 × i8 in 1/64, stored in the tile) is **18–25% smaller than MED** in the hills and
   mountains and 6% smaller on the plains. This changes the candidate codec in the pack
   format.
2. **Entropy coder: static rANS with a shared family of 64 discrete-Laplace models**, one model
   index per tile and activity context (8 contexts). The table family is stored once in the
   section, so the reader computes no floats and builds nothing per tile. A real rANS coder on
   sample tiles came out 1.6–3.1% above the order-0 entropy. Rice coding is within 4% in the
   mountains but 12–180% worse on flat ground (30% at ±1 m); adaptive arithmetic coding gains about 1% over
   static models and decodes slower.
3. **Tiles of 64×64**, aligned to a global 64-sample grid from 90° N, 180° W. Compared with
   128×128 they cost about 2% more bits per sample, but cover 2% fewer samples at the border,
   so the Nepal total is the same (57.9 against 58.0 MB, estimate). A point query decodes at
   most 4,096 samples, and about half that on average, because decoding stops after the rows it
   needs (§5.3).
4. **Error bound: keep ±1 m** (quantisation step 2 m) for both builds, with the step stored per
   grid so the forge can change it without a format change. Coding error adds at most E to any
   point and, in quadrature, about E/√3 to the RMSE: +0.01 m at ±1 m, +0.04 m at ±2 m, +0.17 m at
   ±4 m on the 15.4 m RMSE of S6. ±2 m and ±4 m would be "enough" for the 50 m target, but they
   save only 16 MB and 27 MB on Nepal, and on flat cropland (LE90 4.3 m) ±4 m is the same size as
   the DEM's own error. Accuracy comes first (§11), so ±1 m.
5. **Size of Nepal at ±1 m, 1″, the recommended codec: about 58 MB** (2.57 bits per sample,
   weighted by terrain share; **estimate** from four tiles, §3.6). ±0.5 m: 77 MB, ±2 m: 42 MB,
   ±4 m: 31 MB. The alternatives at ±1 m: MED + rANS 72 MB, JPEG-LS near-lossless 76 MB,
   zstd 19 on LS residuals 77 MB, B1 bit-packing of residuals 102 MB, B1 on row deltas (our
   existing codec unchanged) 124 MB, LERC 146 MB.
6. **Resolution:** keep 1″ in both builds. Dropping to 2″ adds 3.6–3.9 m RMSE of interpolation
   in the hills (P90 6 m) for 21 MB; 3″ adds 6.0–6.5 m RMSE (P90 10 m) for 11 MB. A 3″ grid
   can be offered later as a separate, smaller component; the section format already allows any
   step.
7. **No voids to handle in Nepal:** the four tiles have no NaN and no fill value. Water and
   filled pixels are already heights in GLO-30. An absent tile (outside coverage) reads as
   "no elevation"; a void mask is not in v1.
8. **Heights stay EGM2008 orthometric**, as Copernicus ships them; NASADEM is moved from EGM96 in
   the forge (D-064). Ellipsoidal heights would need the geoid grid in the pack: an open item.
9. **Our own code, no new dependency.** About 400 lines of safe Rust in the reader
   (`forbid(unsafe_code)` holds): rANS decoding, the predictor, the tile walk and bilinear
   sampling. No crate fits: `constriction` is `no_std` but uses `unsafe` internally and is a
   research library; `lerc` wraps C++; `lerc-rs` is pure Rust but LERC is 2.5× larger here.

**Built and measured in Rust, 2026-11-08 (D-104,
[B11](../benches/phase-3.md#b11-dem)).** The codec lands inside this model on all four tiles
(1.112, 2.704, 2.935 and 2.755 bits per sample). Decoding takes 13.6 ns per sample: 31 µs for a
cold point query and 0.4 µs for a warm one. That is slower than the estimate of §5.3, because the
chain from one sample to the next is serial. 32 × 32 tiles answer a cold query in 8.9 µs for
4–8% more bytes, and stay an option per grid. The layout as built is the
[pack-format page](../../04-architecture/pack-format.md#dem); §5.1 below is the draft it came
from. What B11 still has to measure is in §6.2.

## 3. Findings

### 3.1 Data and method

- **Tiles:** `Copernicus_DSM_COG_10_{N26_00_E085_00, N27_00_E085_00, N27_00_E086_00,
  N28_00_E084_00}_DEM.tif` from the URL template of `copernicus-glo30-np` in
  `data/sources.lock`, fetched 2026-11-07. All four SHA-256 match
  `data/sources/copernicus-glo30-np.tiles`. 172 MB in total.
- **What the files are:** float32, 3600×3600, DEFLATE with predictor 3, 1024×1024 internal
  tiles, no `GDAL_NODATA` tag, raster type PixelIsPoint, tie point (0, 0) at the tile's
  north-west corner (for N27 E085: 85.0° E, 28.0° N), 1″ spacing in both axes. Row 0 is the
  northern edge; the southern edge (27.0°) belongs to the next tile down. The values carry
  sub-millimetre fractions. Ranges: 38.9–361.8 m (Terai), 86.5–4461.5 m (mid-hills),
  187.5–8737.8 m (N27 E086), 319.2–8043.8 m (N28 E084).
- **Crop:** 3584×3584 (a multiple of 512) so every tile size divides it. Each figure below is
  over the whole crop (12.8 M samples).
- **Quantisation:** q = round(h / 2E). The maximum error was measured as exactly E for every E.
  Everything after this is lossless on q, so the bound is guaranteed by construction, and
  bilinear interpolation keeps it (a convex combination of values each within E is within E).
- **Cost models:** order-0 entropy per tile; discrete-Laplace (two-sided geometric, "TSG")
  models with θ quantised to 64 levels, per tile or per tile and context, including 6 bits per
  model index; exact Rice cost with the best k per tile; exact B1 cost (9 bytes of base,
  offset and width per 128 values plus 16 bytes per bit of width, D-073); real coders for zstd
  19 (C, 1.5.7), CharLS 2.4.4 (JPEG-LS) and LERC 4.2.0, through `imagecodecs`. Per-tile headers
  (base sample and coefficients, 9 bytes) are included. The directory (4 bytes per tile, 0.008
  bits per sample at 64×64) is added in the Nepal totals.
- **Real rANS check:** a ryg-style rANS (32-bit state, byte renormalisation, 12-bit
  frequencies) with a TSG table was run on 8 tiles of 128×128 per source tile at ±1 m.
- **Round trip:** a sequential decoder written like the planned Rust one (integer prediction,
  the same edge rules) rebuilt q exactly from the residuals and coefficients on sample tiles.
- Scripts and outputs (scratch, not in the repository): `b11.py` (MED coders, B1, zstd,
  JPEG-LS, LERC), `pred2.py` (predictors), `b11v2.py` (LS6 coders by tile size), `realcode.py`
  (round trip, real rANS, C decode times), `jlsnear.py` (JPEG-LS near mode), `resample.py`
  (2″, 3″, 4″), `nptiles.py` (samples and tiles inside Nepal), `taps.py` (predictor order).
  Python 3.15, numpy 2.5.4, tifffile 2026.10.15, imagecodecs 2026.10.10, zstandard 0.25.0.

### 3.2 Predictors

Order-0 entropy of the residual, bits per sample, 128×128 tiles, ±1 m. LS = per-tile least
squares with i8 coefficients in 1/64, header cost included.

| Predictor | Terai | Mid-hills | N27 E086 | N28 E084 |
|---|---:|---:|---:|---:|
| left neighbour | 1.32 | 4.28 | 4.76 | 4.76 |
| MED (LOCO-I) | 1.26 | 3.22 | 3.68 | 3.55 |
| plane (a + b − c) | 1.47 | 2.95 | 3.17 | 2.97 |
| best of MED and plane, per tile | 1.26 | 2.92 | 3.17 | 2.96 |
| LS, 4 taps (a, b, c, d) | 1.19 | 2.84 | 3.11 | 2.92 |
| **LS, 6 taps (+ aa, bb)** | **1.18** | **2.64** | **2.85** | **2.67** |

(The left-neighbour row covers interior samples only; the other rows cover whole tiles with
the edge rules below.)

- MED wins only on the flat Terai, by 6% over the plane; LS6 beats it everywhere.
- More taps help a little: on N27 E086 (interior samples), 6 taps 2.78, 8 taps 2.72, 10 taps
  2.69, 12 taps 2.69 bits. On the Terai all are 1.17–1.18. 6 taps is the recommendation;
  B11 can test 8.
- The predictor is p = a + ((Σ cᵢ·(nᵢ − a) + 32) >> 6) over integers, so encoder and decoder
  agree on every platform. Edge rules inside a tile: row 0 predicts from the left, column 0
  from above, the last column uses b for d, and the first sample is the tile's base.

### 3.3 Entropy coders on the LS6 residuals

Bits per sample, ±1 m, 64×64 tiles (128×128 in brackets where it differs by more than 1%).

| Coder | Terai | Mid-hills | N27 E086 | N28 E084 |
|---|---:|---:|---:|---:|
| order-0 entropy + explicit table per tile | 1.20 | 2.71 (2.65) | 2.96 (2.87) | 2.78 (2.68) |
| TSG, 1 model per tile | 1.20 | 2.71 (2.67) | 2.95 (2.89) | 2.78 (2.71) |
| **TSG, 8 activity contexts per tile** | **1.11** | **2.69 (2.64)** | **2.93 (2.87)** | **2.74 (2.68)** |
| Rice, best k per tile | 1.44 | 2.78 | 3.01 | 2.84 |
| zstd 19 per tile [128] | 1.64 | 3.56 | 3.84 | 3.58 |
| B1 bit-packing of zigzag residuals [128] | 3.24 | 4.51 | 4.82 | 4.74 |

- **Real rANS against the model:** 8 tiles per source tile, ±1 m, TSG table: 1.6% (N27 E086,
  N28 E084), 2.0% (mid-hills) and 3.1% (Terai) above the order-0 entropy. The figures in this
  note add 1.5% for coder overhead.
- **Contexts** (activity |a − c| + |b − c| + |d − b| in 8 bins): 8–16% smaller on flat ground (more at coarser steps),
  1.0–1.4% in the mountains. They cost one comparison chain per sample in the reader.
- **Adaptive coding:** on MED residuals at 128×128 and ±1 m, an adaptive 8-context model (the
  exact Krichevsky–Trofimov cost, which is what an adaptive multi-symbol arithmetic coder
  pays) was 11% below static order-0 on the Terai (1.13 against 1.26) and 1.0–1.5% below in the
  hills and mountains. Static per-context models capture most of that gain (the TSG context row above).
  Adaptive coding needs a division or a model update per sample; not worth it.
- **Huffman** per tile (MED residuals, ±1 m): +17% on the Terai, where entropy is near 1 bit
  and a prefix code cannot go below 1 bit, 1–2% elsewhere.
- **At ±4 m** the explicit per-tile table beats the TSG family by about 4% in the mountains (the
  distribution is less Laplacian when most residuals are 0 or ±1). A family with two shapes
  (TSG and a mixture) is a B11 option.

### 3.4 The other options at the same bound

Bits per sample at 128×128 per tile unless noted.

| Option | Bound | Terai | Mid-hills | N27 E086 | N28 E084 |
|---|---|---:|---:|---:|---:|
| (a) MED + order-0 rANS (static, per tile) | ±1 m | 1.26 | 3.22 | 3.68 | 3.55 |
| (a') LS6 + TSG rANS, 8 contexts, 64×64 (+1.5%) | ±1 m | 1.12 | 2.73 | 2.97 | 2.78 |
| (b) B1 codec, zigzag row deltas (unchanged code) | ±1 m | 3.41 | 5.42 | 6.01 | 6.03 |
| (b) B1 codec, FOR on q | ±1 m | 3.44 | 7.96 | 9.41 | 9.33 |
| (c) LERC 4.2.0 per tile, MaxZError = E | ±1 m | 2.80 | 6.46 | 7.45 | 7.38 |
| (d) zstd 19 on int16 q | ±1 m | 1.60 | 6.82 | 8.43 | 8.22 |
| (d) zstd 19 on row deltas | ±1 m | 1.79 | 4.54 | 5.09 | 4.86 |
| (d) zstd 19 on LS6 residuals | ±1 m | 1.64 | 3.56 | 3.84 | 3.58 |
| (e) JPEG-LS (CharLS) near-lossless, NEAR 3 on a 0.25 m grid | ±0.875 m | 1.37 | 3.49 | 3.85 | 3.66 |

- **(b)** needs no new code, but at 2.1–2.4× the size of (a') it would make the DEM the largest
  part of every pack by far. Bit-packing has no way to spend fewer bits on common values.
- **(c) LERC** quantises each 8×8 or larger block against its minimum and bit-packs; it has no
  pixel predictor, so sloped terrain costs it the full local range: 2.5× (a') on every tile,
  146 MB for Nepal. Its measured maximum error was exactly E. It is the fastest of the C
  decoders here (40–60 µs per 128×128 tile).
- **(d) zstd** finds no 2-D structure: on raw q it does well only on the flat Terai (repeats);
  on the LS6 residuals it is 23–46% above (a'). It also needs the opt-in `ruzstd` (D-079).
- **(e) JPEG-LS** is MED with context modelling, bias correction, adaptive Golomb codes and a
  run mode. Its near-lossless mode (at a slightly tighter ±0.875 m) lands 3–9% above MED's
  order-0 entropy at ±1 m, and 28–32% above (a') in the hills and mountains. CharLS lossless on
  the pre-quantised grid did worse (1.66 on the Terai), probably because a 16-bit input sets
  context thresholds for a far larger range (**unverified**). It is the reference for "MED with
  good context modelling".
- **Decode speed of the C reference codecs**, per 128×128 tile on this M2 Pro (via Python,
  loop overhead included): LERC 40–59 µs, CharLS 160–241 µs, zstd (entropy only, no
  prediction) 20–30 µs.

### 3.5 Error bound against bytes

Bits per sample with the recommended codec (64×64, LS6, 8 contexts, +1.5% coder overhead),
and the effect on accuracy. RMSE effect in quadrature on S6's 15.4 m (all of Nepal) and 5.7 m
(slope 0–5°).

| Bound E (step) | Terai | Mid-hills | Himalaya (mean) | Nepal | +RMSE all | +RMSE flat | Worst case |
|---|---:|---:|---:|---:|---:|---:|---:|
| ±0.5 m (1 m) | 1.70 | 3.61 | 3.79 | 77 MB | +0.003 m | +0.007 m | +0.5 m |
| **±1 m (2 m)** | **1.12** | **2.73** | **2.87** | **58 MB** | **+0.01 m** | **+0.03 m** | **+1 m** |
| ±2 m (4 m) | 0.70 | 1.99 | 2.13 | 42 MB | +0.04 m | +0.12 m | +2 m |
| ±4 m (8 m) | 0.41 | 1.45 | 1.61 | 31 MB | +0.17 m | +0.45 m | +4 m |

- The worst-case column is exact: |Δ + e| ≤ |Δ| + E, so the LE90 of 18.7 m becomes at most
  18.7 + E. The RMSE columns assume the coding error is uniform on [−E, E] and independent of
  the DEM error (**estimate**).
- Each halving of the step costs 0.5–0.9 bits per sample in the mountains and 0.3–0.6 bits on
  flat ground (more at finer steps).
- Every option above is far inside the 50 m budget; the choice is accuracy against megabytes.
  ±1 m keeps the stored value within a tenth of the DEM's error everywhere, including flat
  cropland (LE90 4.3 m), and costs 16 MB more than ±2 m on Nepal.

### 3.6 Tile size and the size of Nepal

- **Samples inside Nepal**, from the `Asia/Kathmandu` polygon of timezone-boundary-builder
  2026f (26,565 vertices; the same outline as the country for this purpose) on an 8″ raster:
  176.4 M at 1″.
- **Tiles that touch Nepal:** 64×64: 43,969 tiles, 180.1 M samples (+2.1%); 128×128: 11,228
  tiles, 184.0 M (+4.3%); 256×256: 2,935 tiles, 192.3 M (+9.1%).
- **Bits per sample by tile size** (LS6, 8 contexts, ±1 m): 64 → 128 saves 1.5–2.2%, 128 → 256
  saves 0.6–1.0%. At 64×64 the per-tile overhead (header 9 bytes, 8 model indices 6 bytes,
  directory 4 bytes) is 0.04 bits per sample.
- **Terrain mix (estimate):** Nepal's physiographic regions are about 14% Terai, 13% Siwalik,
  30% middle mountains, 20% high mountains and 23% high Himal (LRMP 1986, quoted secondhand,
  **unverified**). Weights: Terai tile 14%, mid-hill tile 43%, the mean of the two Himalayan
  tiles 43%.
- **Totals at ±1 m:** 64×64: 2.57 bits × 180.1 M = **57.9 MB**; 128×128: 2.52 × 184.0 M =
  58.0 MB; 256×256: 2.50 × 192.3 M = 60.0 MB. The four tiles are a sample of 45; the real total
  is B11's to measure (§6.2).
- **Directory:** 44 k tile offsets; even unpacked (4 bytes each) that is 176 KB, 0.3% of the
  section.
- **BLOCK_HASHES** (D-078): a 64×64 tile at ±1 m is about 1.3 KiB in the mountains, so a first
  read verifies one 16 KiB block, rarely two.

### 3.7 Resolution: 1″ or coarser

Keep every k-th 1″ sample, bilinear back to 1″, error at the dropped samples against the 1″
values (so this is interpolation error only, on top of the DEM's own error). Size at ±1 m with
LS6 (entropy, no contexts), per 1″ of area.

| Grid | Terai RMSE / P90 | Hills and mountains RMSE / P90 | Max (all tiles) | Bits per 1″ of area (Terai, hills, Himalaya) | Nepal (estimate) |
|---|---|---|---:|---|---:|
| 1″ | 0 | 0 | 0 | 1.12, 2.73, 2.87 | 58 MB |
| 2″ | 0.9 / 1.4 m | 3.6–3.9 / 5.6–6.3 m | 353 m | 0.39, 0.97, 1.04 | 21 MB |
| 3″ | 1.1 / 1.8 m | 6.0–6.5 / 9.7–10.6 m | 486 m | 0.19, 0.52, 0.55 | 11 MB |
| 4″ | 1.3 / 2.1 m | 8.4–9.3 / 14.1–15.2 m | 546 m | 0.11, 0.31, 0.34 | 7 MB |

- In quadrature on the 15.4 m RMSE: 2″ +0.5 m, 3″ +1.2 m, 4″ +2.3 m (hill and mountain RMSE). The P99 at 3″ is 19–20 m in
  the mountains, and the maxima (cliffs in N28 E084) are hundreds of metres.
- NASADEM, the lite build's DEM, is natively 1″, so the lite build has no reason of its own to be
  coarser.

### 3.8 Grid, voids, edges and datum

- **Grid:** GLO-30 widens the longitude spacing with latitude: 1″ to 50°, 1.5″ to 60°, 2″ to 70°,
  3″ to 80°, 5″ to 85°, 10″ to 90° (Copernicus DEM readme on AWS, accessed 2026-11-07). Latitude
  spacing stays 1″. So a section holds one or more grids, each with its own origin and steps,
  in units of 0.1″ (1″ = 10, 1.5″ = 15): no 1″ step is an integer on the 1e-7° coordinate grid
  (ADR-0029).
- **Voids:** none in the four tiles (no NaN, no −32767, no zeros). Ocean has no tiles; the
  readme says to read it as height 0. NASADEM is void-filled. So the section needs no
  in-tile void value in v1. An area with no source tile is an absent tile in the directory, and
  the query returns `None` there.
- **Edges:** tiles do not overlap. A bilinear query at a tile's last row or column needs the
  neighbour tile. At 64×64 that is 3.1% of queries (one neighbour) and 0.02% (three). The
  tile below is needed only for its row 0, which early exit makes cheap. A one-sample overlap
  would cost 3.1% more samples to avoid that; not worth it. At the edge of coverage the query
  clamps to the nearest available sample.
- **Datum:** Copernicus is EGM2008 orthometric; the section stores what the forge's DEM stage
  produces, always EGM2008 (D-063, D-064). The datum and source go in the section's grid
  record, so a reader can say what it returns.
- **Summits:** the maximum of N27 E086 is 8737.8 m; Everest's surveyed height is 8848.86 m. GLO-30
  under-reads that summit by about 111 m, as the timezone-and-elevation note warned (peaks use
  the surveyed height, keeping the DEM height separately).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| (a) quantise + MED + static rANS | Simple, proven (LOCO-I) | 22–33% larger than LS6 in hills and mountains | ours | — | Superseded by (a') |
| **(a') quantise + per-tile LS6 + static rANS, 64 TSG models, 8 contexts** | Smallest measured; integer decoder; no tables built per tile | ~400 lines of our own code; forge fits coefficients | ours | B11 | **Recommended** |
| (a'') as (a') with Rice codes | Simplest decoder, no tables | +30% on flat ground at ±1 m, +180% at ±4 m | ours | — | Fallback if rANS decode is slow |
| (b) B1 bit-packing (row deltas or LS residuals) | Zero or little new code | 2.1× (a') on deltas, 1.7× on residuals | ours | D-073 | Rejected for DEM |
| (c) LERC 2 | Standard (COG, Esri), fast C decoder, pure-Rust `lerc-rs` exists | 2.5× (a') size at ±1 m; no predictor | Apache-2.0 | LERC 4.2.0; `lerc-rs` 0.8.0 (2026-10-29) | Export format only |
| (d) quantise + zstd 19 | Existing opt-in dependency | 1.2–1.5× (a') even on LS residuals; `ruzstd` per tile | BSD/MIT | zstd 1.5.7, `ruzstd` 0.9.1 | Rejected |
| (e) JPEG-LS near-lossless | ISO standard; context modelling and run mode | 1.3× (a') in hills and mountains; `charls` is C++, `pure_jpegls` is lossless only (per its description) | BSD-3 (CharLS) | CharLS 2.4.4; `pure_jpegls` 2.0.0 | Reference only |
| CALIC-style adaptive context coding | Best image-coding ratios | ~1% gain over static per-context models here; slower | — | 1997 | Rejected |

## 5. What we build ourselves, and why

No crate does bounded-error, 2-D predicted, per-tile random-access coding of elevation in safe,
`no_std`-friendly Rust. The parts are small; we build them.

### 5.1 Draft layout (column table, encoding 1; superseded by the built layout, D-104)

`item_count` is the number of tiles. Rows of columns 1–11 are grids (usually one per section);
heights are h = q × step.

| ID | Column | Values |
|---:|---|---|
| 1 | LAT0 | per grid: latitude of the northernmost sample row, in 0.1″, biased (90° S → 0) |
| 2 | LON0 | per grid: longitude of the westernmost sample column, in 0.1″, biased (180° W → 0) |
| 3 | LAT_STEP | per grid: sample spacing in latitude, 0.1″ (10 for 1″) |
| 4 | LON_STEP | per grid: sample spacing in longitude, 0.1″ (10, 15, 20, 30, 50 or 100) |
| 5 | TILE_ROWS | per grid: rows of tiles |
| 6 | TILE_COLS | per grid: columns of tiles |
| 7 | TILE_START | per grid and one more: its first tile in OFFSETS |
| 8 | STEP | per grid: the height step in millimetres (2000 for ±1 m) |
| 9 | DATUM | per grid: 1 EGM2008 orthometric (others reserved) |
| 10 | SOURCE | per grid: an index into the pack's source list (manifest) |
| 11 | TILE_LOG2 | per grid: log2 of the tile side (6 for 64×64) |
| 12 | OFFSETS | per tile and one more: byte offset of its code in PAYLOAD; equal neighbours mean an absent tile |
| 13 | MODELS | the model family: 64 models × 64 symbols, 12-bit frequencies (cumulative), symbol 63 the escape |
| 14 | PAYLOAD | raw bytes (codec 2): the tiles back to back |

- Tile code: base sample (zigzag LEB128), 5 coefficients (i8), 8 model indices (6 bits each,
  6 bytes), then the rANS stream; an escaped residual (|r| > 31) follows as 16 raw bits.
  B11 decides whether the header moves into columns instead.
- The global tile grid is anchored at 90° N, 180° W in tile units, so neighbouring countries'
  packs share identical tile codes, and a release that does not change the source keeps the
  section's bytes (the stable-section-bytes rule).
- The tile with constant q (a flat lake) is a header with model index "all zero".

### 5.2 Writer (forge)

- Stream the source tiles in bands of 64 rows across the pack's width; bounded memory (a band
  of Nepal is 64 × 29,000 samples).
- Quantise; fit LS6 by exact integer normal equations (i64 sums), solve in f64 with a fixed
  operation order and no libm call, round to i8; the result is deterministic on IEEE hardware.
  Any coefficients decode correctly, so determinism matters only for reproducible bytes.
- Pick each context's model by integer comparisons of Σ|r| / n against stored thresholds, not by
  float `pow`. The 64-model family is generated once and checked in; the reader reads it from
  MODELS and never computes it.
- Encode rANS backwards per tile; write OFFSETS with the B1 codec.

### 5.3 Reader

- `elevation(lat, lon, &mut DemScratch) -> Option<f32>`: find the grid; compute the sample row,
  column and fractions; decode the tile(s) into a caller-owned scratch (`[i16; 4096]` per tile,
  8 KiB); bilinear; × step. No allocation, `forbid(unsafe_code)`, every length checked against
  the section (a fuzz target).
- **Early exit:** decoding runs forward in raster order, so it stops after row y + 1 of the
  tile: on average about 2,100 of 4,096 samples. Restart points inside a tile do not help,
  because each row is predicted from the rows above it.
- **Cache (optional):** a caller-owned fixed array of N decoded tiles with their IDs, for
  batch and path queries; a batch API sorts points by tile.
- **Speed, measured in B11:** 13.6 ns per sample, 56 µs per whole tile, 31 µs per cold
  query (about half a tile), 0.43 µs warm. Two changes gave the last 30%: neighbours rolled in
  registers inside a row, and a 64-bucket start for the symbol search. The original estimate
  follows.
- **Speed (estimate, unverified):** 3–6 ns per sample (rANS step plus 5 multiply-adds on a
  dependency chain) gives 6–12 µs per early-exit tile and 12–25 µs for a full one. LERC's C
  decoder takes 40–60 µs for a 128×128 tile (§3.4), which is 10–15 µs per 4,096 samples.

## 6. Impact on the plan

### 6.1 Decisions to record

- **D-new (DEM codec):** DEM tiles are 64×64 on a global grid; quantised to a step of 2E with
  E = 1 m; predicted by per-tile 6-tap least squares with i8 coefficients; coded with static
  rANS over a 64-model discrete-Laplace family stored in the section, 8 activity contexts per
  tile. Measured on four Copernicus tiles: 1.12–2.97 bits per sample, about 58 MB for Nepal
  (estimate). This replaces "MED prediction" in the pack-format candidate column.
- **D-new (bound and resolution):** ±1 m and 1″ in both builds; the step and grid are per-grid
  parameters of the section, so a coarser component (3″, about 11 MB for Nepal) can be added
  without a format change.
- [Pack format](../../04-architecture/pack-format.md) kind 13: content and codec cells change;
  the DEM column table above becomes its draft section page with B11.
- [Format, compression and indexing §3.8](format-compression-indexing.md#38-dem-raster-compression-at-a-1-m-bound):
  its estimate (1.5–3 bits per pixel) is confirmed for MED in the mountains only; its 256×256
  tile size and MED predictor are superseded by this note.
- Open question: ellipsoidal heights (WGS84) need the EGM2008 undulation in the pack. A 5′ grid
  over Nepal is about 4,600 values; it could be a second grid in this section with its own step
  (for example 1 cm). Not decided here.

### 6.2 What B11 must measure in Rust

Done in B11: the four tiles in Rust (item 1 in part), decode time on M2 Pro cold, warm and
whole (item 2 in part), the symbol lookup (item 3: a bucket table, not a binary search; one
state, because the chain is serial), writer determinism and read-back (item 5 in part), and
the planted-bug proofs of the new tests (item 7). Thresholds and refinement were measured too
([B11](../benches/phase-3.md#b11-dem)).

1. All 45 Nepal tiles (and 2–3 non-Nepal tiles: a desert, a coast, a tile above 50° with
   1.5″ spacing): bits per sample and the Nepal total, against the 58 MB estimate.
2. Decode time per point query (random points, cold and with a cache), per full tile and per
   early-exit tile, on M2 Pro, wasm (Node) and one Android device class; target to be set by
   the result, compared with LERC through `lerc-rs`.
3. rANS symbol lookup: binary search in cumulative frequencies against a 4096-slot table built
   per used model; one or two interleaved states.
4. 6 against 8 taps; 8 against 4 contexts; one TSG family against TSG plus a second shape
   (§3.3, ±4 m).
5. Writer determinism: two builds byte-identical; the same tile bytes in two packs that share a
   tile; forge throughput and peak memory for Nepal.
6. Accuracy gate: the S6 run repeated on the decoded section, LE90 within 18.7 m + 1 m, and the
   max error ≤ E over every sample (a deep check).
7. Prove each check fails on a deliberate violation (a wrong coefficient, a step of 2E + 1).

## 7. Risks and open items

- **Four tiles are a sample.** The Nepal size is an estimate weighted by an unverified terrain
  split. B11 measures all 45.
- **Decode speed** on M2 Pro is 31 µs per cold query (B11). Phones and wasm are not measured
  yet. If 64×64 is too slow there, 32×32 tiles are measured: 8.9 µs for 4–8% more bytes.
- **Laplace model mismatch** costs about 4% at ±4 m; harmless at ±1 m.
- **Copernicus licence:** its notice must ship in every main pack (ADR-0030); the lite build uses
  NASADEM. Unchanged by this note.
- **Tiles above 50°** with 1.5″–10″ longitude spacing are untested; the grid record handles them,
  the predictor may need different taps where samples are not square.
- **`constriction`** (0.5.0) was checked as an rANS source: `no_std` with `default = ["std"]`,
  MIT / Apache-2.0 / BSL-1.0, but it declares `unsafe` traits and functions, and a research API
  that is larger than the 100 lines we need. Not used.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Copernicus GLO-30 COG tiles (4) | GLO-30 Public DGED, SHA-256 pinned | 2026-11-07 | `data/sources/copernicus-glo30-np.tiles` |
| `constriction` | 0.5.0 (2026-07-17) | 2026-11-07 | crates.io |
| `lerc` / `lerc-sys` (C++ wrapper) | 0.2.1 (2025-06-20) / 0.1.5 | 2026-11-07 | crates.io |
| `lerc-rs` (pure Rust, Apache-2.0, MSRV 1.88) | 0.8.0 (2026-10-29) | 2026-11-07 | crates.io |
| `lerc-reader` (pure Rust decoder) | 0.5.2 (2026-10-31) | 2026-11-07 | crates.io |
| `tiff` (pure Rust) | 0.11.3 (2026-02-10) | 2026-11-07 | crates.io |
| `rans` / `ryg-rans-sys` | 0.4.0 / 1.2.0 (2024-02-08) | 2026-11-07 | crates.io |
| `ans` | 0.5.0 (2026-11-04) | 2026-11-07 | crates.io |
| `huffman-compress` | 0.7.0 (2025-12-16) | 2026-11-07 | crates.io |
| `charls` / `charls-sys` | 0.4.2 / 2.4.6+charls.2.4.5 | 2026-11-07 | crates.io |
| `pure_jpegls` | 2.0.0 (2026-07-20) | 2026-11-07 | crates.io |
| `ruzstd` | 0.9.1 (2026-10-11) | 2026-11-07 | crates.io |
| LERC (C++) | 4.2.0 (in imagecodecs), Apache-2.0 | 2026-11-07 | github.com/Esri/lerc |
| CharLS | 2.4.4 (in imagecodecs) | 2026-11-07 | imagecodecs 2026.10.10 |
| zstd (C) | 1.5.7 | 2026-11-07 | imagecodecs, zstandard 0.25.0 |

## 9. Sources

- [Copernicus Digital Elevation Model datasets (readme)](https://copernicus-dem-30m.s3.amazonaws.com/readme.html),
  AWS open data, accessed 2026-11-07: latitude bands and longitude spacing, ocean without tiles.
- [Copernicus DEM Product Handbook, issue 5.0](https://dataspace.copernicus.eu/sites/default/files/media/files/2024-06/geo1988-copernicusdem-spe-002_producthandbook_i5.0.pdf),
  Airbus / ESA, 2022, accessed 2026-11-07 (grid spacing table; not re-read in full).
- M. J. Weinberger, G. Seroussi, G. Sapiro, "The LOCO-I lossless image compression algorithm:
  principles and standardization into JPEG-LS", *IEEE Trans. Image Processing* 9(8), 2000.
- X. Wu, N. Memon, "Context-based, adaptive, lossless image coding" (CALIC), *IEEE Trans.
  Communications* 45(4), 1997.
- J. Duda, "Asymmetric numeral systems: entropy coding combining speed of Huffman coding with
  compression rate of arithmetic coding", arXiv:1311.2540, 2013.
- [ryg_rans](https://github.com/rygorous/ryg_rans), F. Giesen, public domain reference rANS,
  accessed 2026-11-07.
- [Esri LERC](https://github.com/Esri/lerc), Apache-2.0, accessed 2026-11-07.
- [lerc-rs](https://github.com/NathanHowell/lerc-rs) README on crates.io, accessed 2026-11-07
  (claims pure Rust, no unsafe, `no_std` + `alloc`; **unverified** by us).
- [constriction](https://github.com/bamler-lab/constriction) `src/lib.rs` and `Cargo.toml`,
  accessed 2026-11-07.
- crates.io API for every crate in §8, accessed 2026-11-07.
- timezone-boundary-builder 2026f `combined-with-oceans.json` (`data/cache/tbb-2026f`), for the
  Nepal outline.
