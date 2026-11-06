---
title: "Spike S6: the DEM against ICESat-2 ground heights over Nepal"
status: research
updated: 2026-11-07
---

# Spike S6: the DEM against ICESat-2 ground heights over Nepal

**Question:** how much of the vertical target (P90 ≤ 50 m, D-062) does the DEM itself use,
and where? The method and the choice of reference are in the
[elevation-reference note](../notes/elevation-reference.md); the stage is described in
[forge](../../04-architecture/forge.md#dem-against-a-ground-reference-dem-ref-and-the-geoid).

**Inputs** (all in `data/sources.lock`):
- Copernicus GLO-30, 45 tiles over N26–N30, E080–E088 (`copernicus-glo30-np`);
- ICESat-2 ATL08 release 007, 2023, the same 45 tiles, through SlideRule with terrain quality
  ≥ 70 (`sliderule-atl08-np-2023`, 316 MB);
- the EGM2008 5′ geoid (`egm2008-5`);
- COD-AB Nepal, for the run restricted to Nepal.

Run 2026-11-07, M2 Pro:

```
forge dem-ref copernicus-glo30-np sliderule-atl08-np-2023 egm2008-5 [codab-np-geojson]
```

The box run takes 4.8 s for 3,251,833 segments with a peak RSS of 594 MB. The Nepal run takes
4.4 s and 625 MB. Both summaries are byte-identical on a second run.

## Checks before the numbers

- **Datum:** ATL08 heights are ellipsoidal. With the geoid switched off, the bias on flat
  cropland is +65.5 m and in the 0–500 m band +65.7 m: that is N ≈ −66 m in the Terai, the value
  the timezone-and-elevation note predicts. With the geoid the same strata show +0.4 m and
  +0.7 m, so the conversion is right.
- **Geoid:** `forge geoid` agrees with GeographicLib's online egm2008-1 values within 0.12 m at
  five points.
- **Incomplete responses:**
  - A SlideRule response with a server error is rejected, both at fetch and in the stage
    (proven on a deliberately bad field name: level 3, codes −7 and −1).
  - Two tiles reported errors on every attempt. In both, a granule has no `land_segments` for
    beams gt3l and gt3r, which the `h5p` endpoint confirmed (the other four beams exist).
  - `forge fetch` accepts such a beam only after that confirmation; an existing beam that fails
    is retried. The test against the live service distinguishes the two (`beam_absent`).

## Results inside Nepal (COD-AB)

Δ = Copernicus − ATL08 ground, EGM2008 orthometric, metres. Bias is the median Δ; LE90 is the
P90 of |Δ|.

| Stratum | Segments | Bias | RMSE | LE90 | P99 \|Δ\| | > 50 m |
|---|---:|---:|---:|---:|---:|---:|
| **all** | 497,717 | +2.6 | 15.4 | **18.7** | 55.9 | 1.28% |
| slope 0–5° | 137,881 | +0.4 | 5.7 | 6.9 | 19.3 | 0.09% |
| slope 5–15° | 67,967 | +3.1 | 12.7 | 16.9 | 39.1 | 0.59% |
| slope 15–30° | 157,988 | +4.3 | 14.4 | 18.8 | 48.9 | 0.95% |
| slope ≥ 30° | 133,881 | +6.4 | 22.8 | 27.7 | 83.7 | 3.26% |
| forest | 201,042 | +8.8 | 16.1 | 22.0 | 50.1 | 1.01% |
| cropland | 123,561 | +0.4 | 5.5 | 4.3 | 15.1 | 0.11% |
| shrub and grass | 81,461 | +1.6 | 15.4 | 16.3 | 64.2 | 1.76% |
| built-up | 5,080 | +1.6 | 6.6 | 4.2 | 14.3 | 0.06% |
| bare | 59,932 | +0.9 | 19.1 | 17.2 | 81.7 | 2.67% |
| snow and ice | 23,277 | +3.2 | 29.7 | 32.6 | 112.4 | 5.08% |
| night | 254,138 | +2.4 | 13.7 | 16.7 | 48.7 | 0.95% |
| day | 243,579 | +3.0 | 17.0 | 20.8 | 63.1 | 1.63% |
| height 0–500 m | 151,421 | +0.7 | 8.2 | 13.2 | 24.7 | 0.13% |
| height 500–1,500 m | 109,556 | +6.2 | 14.1 | 19.5 | 42.5 | 0.68% |
| height 1,500–3,000 m | 77,274 | +6.5 | 16.2 | 22.5 | 56.6 | 1.39% |
| height 3,000–4,500 m | 54,124 | +3.6 | 18.8 | 24.5 | 70.3 | 2.35% |
| height ≥ 4,500 m | 105,342 | +1.2 | 21.0 | 20.9 | 85.8 | 2.95% |

The whole box, including the Tibetan plateau and the Indian plains (3,251,827 segments), gives
LE90 6.2 m, bias +0.4 m and 0.29% above 50 m. Nepal is harder than the region around it.

## Findings

- **The DEM meets the vertical target on its own everywhere in Nepal:** LE90 18.7 m overall,
  and 32.6 m in the worst stratum (snow and ice). Assuming independent errors, about
  √(50² − 18.7²) ≈ 46 m of the P90 budget is left for position. That is what D-062's targets
  assume.
- **Forest is a surface-model bias, not noise:** +8.8 m median, as expected for X-band radar
  over canopy. It explains the +6 m bias in the 500–3,000 m bands, Nepal's forested hills.
  Correcting it with a canopy-height layer, or using a bare-earth DEM, is the S6.3 comparison
  (GEDTM30).
- **Slope and snow drive the tail:** above 30° the LE90 is 27.7 m and 3.3% are off by more than
  50 m; on snow and ice 5.1% are. Part of this is ATL08's own geolocation (about 5 m, **unverified**) times
  the slope, and glacier change since the DEM's 2011–2015 acquisitions.
- **Night is better than day,** LE90 16.7 m against 20.8 m: daytime solar background adds noise
  photons.
- **σ_DEM prior:** the note's prior, 2 m + 0.5 m per degree of slope, gives about 21 m at 38°.
  The measured LE90 of 27.7 m above 30° is σ ≈ 17 m. The prior is the right order and slightly
  conservative. Fitting it to the per-point output is Phase 2 work.

## What follows (D-063)

- ATL08 through SlideRule is the elevation reference for every country: one lock entry per
  country and year, the same stage.
- Copernicus GLO-30 stays the main-build DEM. The vertical budget in hills and mountains is
  spent mostly by position (S5.6, D-062), not by the DEM.
- The per-point output (`data/build/dem-ref/…/<tile>.parquet`) is the calibration set for σ_DEM
  and for the forest correction.
