---
title: "Elevation reference: ICESat-2 ATL08 through SlideRule, and the EGM2008 geoid"
status: research
updated: 2026-11-07
owner: forge
revisit: 2027-02-07
---

# Elevation reference: ICESat-2 ATL08 through SlideRule, and the EGM2008 geoid

## 1. Question

Spike S6 measures the DEM part of the vertical target (P90 ≤ 50 m, D-062). It needs:
- an independent ground reference that any country can use;
- a way to fetch that reference that pins and reproduces with no account;
- a way to put the reference and the DEM on one vertical datum.

The [timezone and elevation note](timezone-and-elevation.md) chose ICESat-2 ATL08 as the
reference (§3.B and the G-ELEV-P90 gate). This note settles how we get it and how we convert
its heights.

## 2. Summary and recommendation

- **Fetch ATL08 through SlideRule's public `atl08x` endpoint.** It needs no account or token:
  the endpoint is `signed = false` with no roles, and a request on 2026-11-07 succeeded with
  no credentials. It subsets granules by polygon and time on the server and returns GeoParquet
  (zstd) inside its record stream. One 1° × 1° tile for 2023 (N27 E085) is 4.9 MB, 35,079
  segments, in 12 s.
- **Pin it as a tileset** (D-061): one POST per 1° tile, the request body rendered from a
  template in `forge/queries/`. The stored bytes are the raw response stream, so the store
  pins exactly what came back. The response is not byte-stable between runs (segments arrive
  asynchronously), so the source is `moving` and is re-pinned only on purpose.
- **Geoid: EGM2008 from GeographicLib's 5′ grid (`egm2008-5`),** bilinear. Interpolation error
  is RMS 12 mm and at most 0.478 m, below the budget by two orders of magnitude. ATL08 heights
  are WGS84 ellipsoidal (ITRF2014), and Copernicus is EGM2008, so H = h − N.
- **Result (S6.2, 2026-11-07):** Copernicus against 497,717 ATL08 segments inside Nepal has
  LE90 18.7 m, and at most 32.6 m in any stratum
  ([S6](../benches/s6-elevation.md)).
- **Empty beams:** a granule can lack `land_segments` for a beam, and SlideRule reports that
  as a read error. The fetch accepts it only after the `h5p` endpoint confirms the group is
  absent (D-063).
- **Compare at ATL08's own point:** `h_te_best_fit` (the fitted terrain at the segment
  centre) minus N, against Copernicus bilinear at the same point. Stratify by slope, land
  cover, snow and day or night. Drop fill values.

## 3. Findings

**SlideRule** (server v5.8.0, cluster launched 2026-11-06; client release v5.8.1 of
2026-11-04):
- `POST https://sliderule.slideruleearth.io/source/atl08x` with `{"parms": {...}}`. Parameters
  used: `poly` (lon/lat ring), `t0`, `t1`, `te_quality_filter` (minimum terrain quality score),
  `atl08_fields` (paths under `land_segments/`, e.g. `terrain/h_te_best_fit`) and
  `output: {format: "parquet"}`.
- **Response:** a stream of records. Each has a big-endian header (i16 version = 2, i16 type
  length, i32 data length), then a NUL-terminated type name, then the data.
  - `arrowrec.meta` holds the file name (128 bytes) and size (i64 LE at offset 128).
  - `arrowrec.data` holds the file name (128 bytes) and then a chunk of the Parquet file.
  - `arrowrec.eof` holds a checksum.
  - `exceptrec` holds server log lines (i32 code, u32 level at 32, text at 64).
  - Layouts are from `GET /source/definition {"rectype": ...}` and match the Python client's
    parser.
- **Columns** include `latitude`, `longitude` (EPSG:7912), `h_te_median`, `h_te_uncertainty`,
  `terrain_slope`, `n_te_photons`, `segment_landcover`, `segment_snowcover`,
  `solar_elevation`, `te_quality_score`, `srcid`, `gt`, `cycle`, `rgt` and the requested
  `terrain/h_te_best_fit`. A wrong field path fails that beam only, with an `exceptrec` that
  names it.
- Some segments carry fill values (3.4e38 or NaN): in the test tile, 24,402 of 35,079
  segments were valid. `h_te_uncertainty` has a median of 46 m in the hills. It includes
  systematic terms, so it is a stratum, not a filter.
- **Plausibility:** Kathmandu valley ground reads `h_te_median` 1,226 m and up, ellipsoidal.
  With N ≈ −40 m that is about 1,266 m orthometric, consistent with the valley floor.

**ICESat-2 data licence:** NASA Earth science data carry no restriction on use (NASA Earth
Science Data and Information Policy). Class `public-domain`. Credit to NASA NSIDC DAAC and
SlideRule (University of Washington and NASA GSFC) is given.

**GeographicLib geoid grids** (geoid documentation, accessed 2026-11-07):

| Grid | Spacing | Size | Bilinear max | Bilinear RMS |
|---|---|---|---:|---:|
| egm2008-5 | 5′ | 19 MB (17 MB zip) | 0.478 m | 12 mm |
| egm2008-2_5 | 2.5′ | 75 MB | 0.135 m | 3.2 mm |
| egm2008-1 | 1′ | 470 MB | 0.025 m | 0.8 mm |

- 16-bit PGM with `Offset` and `Scale` header comments: N = offset + scale · pixel
  (3 mm quantisation).
- Rows run from 90° N down to 90° S (h rows, spacing 180°/(h − 1)); columns from 0° E
  eastwards (w columns, spacing 360°/w, wrapping).
- The data are computed from NGA's EGM2008 grids. NGA released EGM2008 for public use. The
  exact licence wording is **unverified**; GeographicLib itself is MIT.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| SlideRule `atl08x` (public) | No account; server-side subsetting; Parquet out; any region | Third-party service; output not byte-stable; rate limits unknown | NASA data, no restriction; SlideRule BSD-3 | v5.8.0, 2026-10-29 | **Use** |
| NSIDC / Earthdata ATL08 HDF5 granules | Primary source; byte-stable granules | Needs an Earthdata login (the maintainer's account); hundreds of MB per granule; an HDF5 reader (the `hdf5` crate links libhdf5) | NASA data, no restriction | ATL08 release 007 | Later, for a release mirror |
| NSIDC OpenAltimetry | Simple subsets | Moved under NSIDC with login; interactive-oriented | as above | | No |
| GEDI L2A (lidar, ±51.6°) | Dense, ground returns | Not polar; also via SlideRule; canopy-tuned | NASA | | A second reference later |

| Geoid option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| egm2008-5, bilinear | 17 MB, trivial code, 12 mm RMS | 0.48 m max | **Use** |
| egm2008-2_5 or -1 | Smaller error | 75–470 MB for no measurable gain at a 50 m budget | Only if a later gate needs centimetres |
| Spherical-harmonic evaluation | Exact | 2190-degree evaluation per point; slow and complex | No |

## 5. What we build ourselves, and why

- **A POST tileset** in `forge fetch`: the existing tileset path (D-061) gains a request body
  template, so a server-side subsetting API pins per tile like a COG. It is world-general;
  only the lock entry is regional.
- **A SlideRule stream decoder** (about 50 lines): it extracts the Parquet bytes from the
  `arrowrec.data` records and reads them with the `parquet` crate already in the forge.
- **A geoid module:** PGM parse plus bilinear interpolation, with a test against
  GeographicLib's published example values.
- **A reference stage** that samples a DEM tileset at every reference point and reports
  |ΔH| by slope, land cover, snow and day or night.

## 6. Impact on the plan

- Phase 0 S6 starts with Copernicus against ATL08 over the Nepal box. NASADEM needs an
  Earthdata login (LP DAAC) or an OpenTopography key, and GEDTM30 is one global COG on Zenodo.
  Both come after the Copernicus measurement: NASADEM when the maintainer provides access,
  GEDTM30 through range reads of its COG.
- σ_DEM priors (timezone and elevation note §3.B.1) are calibrated from this measurement.

## 7. Risks and open items

- SlideRule's public cluster may rate-limit or change. Mitigations: fetch sequentially, pin
  the bytes, and mirror them with the first data release.
- The EGM2008 grid's licence wording is **unverified**. Check NGA's statement before the grid
  enters a pack; for measurement it is only used in the forge.
- ATL08's geolocation error (about 5 m horizontal, **unverified**) adds σ_h·tan(slope) on steep ground. It is
  reported by slope band, not corrected.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| SlideRule server | v5.8.0 | 2026-11-07 | `/source/version` |
| SlideRule client | v5.8.1 | 2026-11-07 | GitHub releases |
| ATL08 | release 007 | 2026-11-07 | granule names in the response |
| GeographicLib geoids | egm2008-5 | 2026-11-07 | geoid documentation |

## 9. Sources

- [SlideRule ICESat-2 user guide](https://github.com/SlideRuleEarth/sliderule/blob/main/docs/user_guide/icesat2.md), accessed 2026-11-07.
- [SlideRule `atl08x` endpoint](https://github.com/SlideRuleEarth/sliderule/blob/main/apps/node/datasets/icesat2/endpoints/atl08x.lua) and `Atl08DataFrame.cpp`, accessed 2026-11-07.
- [SlideRule Arrow output](https://github.com/SlideRuleEarth/sliderule/blob/main/docs/user_guide/arrow_output.md), accessed 2026-11-07.
- [SlideRule JOSS paper](https://joss.theoj.org/papers/10.21105/joss.04982.pdf), 2023.
- [NASA ICESat-2 data](https://icesat-2.gsfc.nasa.gov/icesat-2-data), accessed 2026-11-07.
- [GeographicLib geoid height](https://geographiclib.sourceforge.io/C++/doc/geoid.html), accessed 2026-11-07.
