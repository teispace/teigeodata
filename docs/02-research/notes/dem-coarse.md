---
title: "A coarse terrain grid for phones (TG-10)"
status: accepted
updated: 2026-11-12
owner: maintainer's agents
revisit: 2027-05-12
---

# A coarse terrain grid for phones (TG-10)

## 1. Question

A consumer (Tithipatra, a calendar computing sunrise offline) wants terrain heights at any
point without the 1″ DEM, which is 68 MB on the wire for Nepal. They asked for a grid of about
15″ (about 460 m), or 30″ where that meets the budget, at 1 MB or less for Nepal, answered by
`elevation_at` with its resolution stated. Which grid, how it is made, and how it is gated
(D-225).

## 2. Summary and recommendation

A new section `DEM_COARSE` with the DEM's own layout and coder, in its own component
`dem-coarse`. The forge makes it from the pack's own 1″ DEM by block means over 15″ cells,
heights quantised to 8 m, tiles of 32. On Nepal it is **517,997 B on the wire** (budget
1,000 kB) and reads within **P50 15.9 m, P90 61.0 m, P99 109.8 m** of the 1″ DEM at the
pack's 95,723 places. `elevation_at` uses it where no fine DEM answers.

## 3. Findings

### 3.1 How much height error the use tolerates

The dip of the sea horizon is `√(2h/R)` radians for an observer at height `h` (R ≈ 6,371 km).
At 1,400 m it is 1.201°; at 1,700 m 1.324°: 300 m of height error moves the dip 0.123°. The
sun's altitude near the horizon changes at about 15°/h × cos φ × cos δ, 13.2°/h at 28° N near
an equinox, so 0.123° is 34 s: the consumer's "half a minute" holds. A P99 of 110 m is about
12 s of sunrise. (Terrain horizons, not the sea horizon, govern sunrise in the hills; that is
the consumer's model, not this grid's.)

### 3.2 How coarse grids are made elsewhere

USGS GMTED2010 makes its 7.5″, 15″ and 30″ products from finer DEMs by several aggregations
over each cell: minimum, maximum, mean, median, standard deviation, systematic subsample and
breakline emphasis (Danielson and Gesch 2011). Its mean product is the general-purpose one;
the subsample keeps one source sample per cell and so carries that sample's local noise.
GTOPO30 (1996) was the earlier 30″ global grid. **Unverified here:** GMTED's published RMSE
per product; we measured our own error instead (§4).

### 3.3 The error a coarse grid must make

A coarse sample is one number for a cell of 15 × 15 1″ samples. Read at a place, the error
is how far the terrain at the place stands from the cell's mean (plus bilinear blending with
the neighbouring cells). So the predicted error at a place is σ, the standard deviation of the
1″ samples in its cell; the gate holds the measured error to it.

## 4. Options compared

Measured on Nepal lite (`np-places-lite.tgd`, NASADEM 1″, 2026-11-12), every place but zone
areas, `forge dem-coarse <pack> --arcsec A --step-m S --log2 L`. Wire is zstd 19 of the
section.

| Grid | Step | Tiles | Wire B | P50 / P90 / P99 m | Predicted σ P50 / P90 / P99 m |
|---|--:|--:|--:|---|---|
| 15″ | 2 m | 32 | 1,134,644 | 15.9 / 61.0 / 109.6 | 42.1 / 71.6 / 102.0 |
| 15″ | 4 m | 32 | 729,200 | 15.9 / 61.0 / 109.8 | same |
| **15″** | **8 m** | **32** | **517,965** | **15.9 / 61.0 / 109.8** | same |
| 15″ | 8 m | 16 | 560,773 | same | same |
| 15″ | 8 m | 64 | 512,591 | same | same |
| 30″ | 2 m | 32 | 387,656 | 34.2 / 120.9 / 215.9 | 77.8 / 129.5 / 182.5 |
| 30″ | 8 m | 32 | 176,066 | 34.2 / 121.0 / 215.5 | same |

- **Step:** the cell's spread dwarfs the quantisation (±4 m at 8 m): the percentiles move at
  most 0.2 m from 2 m to 8 m, for 54% fewer bytes.
- **Spacing:** 30″ doubles the error for a third of the bytes. Nepal meets its budget at 15″,
  so 15″ is the grid; a country that does not can take 30″ (a per-country choice when one
  needs it).
- **Tile side:** 64 saves 1% over 32 but lets the filled edge of a tile reach 16′ past the
  fine grid instead of 8′; 16 costs 8% more.
- **Aggregation:** block mean, as GMTED's general product. A subsample keeps one 1″ sample's
  noise per cell and a median needs a sort per cell; neither was measured, since the mean is
  the least-squares estimate of the cell and its measured error already sits under the
  prediction.

## 5. What we build ourselves, and why

No coder to choose: the DEM's own (D-104) codes the coarse grid as well, so one reader serves
both kinds. `dem::coarsen` streams one coarse tile at a time and decodes only the 1″ tiles
under it: 6–7 s for Nepal, 134 MB peak footprint with the 80 MB pack read whole.

G-dem-coarse, on every pack the forge writes and in `forge dem-coarse`:

1. P90 of |coarse − 1″| at most 1.0 × P90 of σ plus half a step (measured ratio 0.85 at 15″,
   0.93 at 30″);
2. every place with a 1″ height has a coarse one;
3. each of the places' cells, read at its coarse sample, within half a step (+0.25 m of
   interpolation) of the mean of its 1″ samples, summed apart from the builder.

Check 3 exists because check 1 misses biases: places sit on ridges and valley floors, so
raising every coarse height by 16 m moved P90 by −2.6 m. Planted: +8 m fails 85,773 of
88,611 cells, +16 m all of them; the unit tests plant +2 m, +26 m and a cut-away tile.

## 6. Impact on the plan

D-225; [pack format](../../04-architecture/pack-format.md#dem) (kind 24, component
`dem-coarse`); [engine](../../04-architecture/engine.md) (`elevation_at` falls back, feature
`dem-coarse`); [sizes](../../05-data/sizes.md); phase 4 §4.9c TG-10; Nepal's `wire_kb`.

## 7. Risks and open items

- A filled cell outside the fine grid answers from the nearest cell (at most 8′ past it):
  inside a layer's box, where `elevation_at` asks, a point a little beyond the fine DEM's
  halo reads an extrapolated height.
- The tithipatra profile does not list `dem-coarse` until Nepal's pack is rebuilt with it.
- Sizes for India and other countries come with their builds.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| NASADEM (the lite build's DEM) | as pinned in `data/sources.lock` | 2026-11-12 | the Nepal lite pack |
| GMTED2010 | Open-File Report 2011–1073 | 2026-11-12 | USGS |

## 9. Sources

- Danielson, J.J., and Gesch, D.B., 2011, *Global multi-resolution terrain elevation data
  2010 (GMTED2010)*: U.S. Geological Survey Open-File Report 2011–1073,
  <https://pubs.usgs.gov/of/2011/1073/>.
- USGS EROS, *GTOPO30*, <https://www.usgs.gov/centers/eros/science/usgs-eros-archive-digital-elevation-global-30-arc-second-elevation-gtopo30>.
- The dip of the horizon and the sun's rate of rise: standard spherical astronomy, e.g.
  Meeus, *Astronomical Algorithms*, 2nd ed., 1998.
- [The DEM section](dem-section.md) (D-089, D-104, D-192, D-195).
