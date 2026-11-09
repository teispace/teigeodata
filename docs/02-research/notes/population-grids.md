---
title: "Population grids for admin units"
status: research
updated: 2026-11-09
owner: forge
revisit: 2027-05-01
---

# Population grids for admin units

## 1. Question

Nepal's 753 local units and most districts have no population in any pinned source: COD-AB
carries none, Wikidata has P1082 on few municipality items, and the census tables wait on
Q-011. Search importance (D-052) adds 5·log10(population), so a municipality scored 45 + match
while a GeoNames point of the same name, with a population, scored more: Kamalamai, Damak and
Bhimeshwor found the point first, and a district one edit away beat an exact municipality
(Kailari, Myagde). Which world-wide source gives every admin unit a population, under which
licence, and how accurate is it? Feeds D-123 and phase 3 (search ranking).

## 2. Summary and recommendation

Sum **WorldPop Global2 R2025A** (constrained, 2025, 3 arc-seconds) over each unit. It matches
Nepal's 2021 census at province level within 1.8 % (median; P90 2.2 %) and districts within
4.5 % (median), against GHSL GHS-POP R2023A's 20.2 % and 17.6 %. Kathmandu Metropolitan City
comes out at 863,151 (census 2021: 845,767), Lalitpur at 294,686 (294,098). A stated count
from a linked record stays when the modelled one is within 1.5×; otherwise it is replaced
(about half the stated municipal counts are a district's carried onto a same-named unit).
Golden set on the product: recall@1 0.799 → 0.852, pass 0.887 → 0.918, no class worse but
colloquial (one query, Patan; §7).

Licence: CC BY 4.0, but WorldPop's covariates may include OpenStreetMap (**unverified**), so
main build only (ADR-0030) until the inputs are confirmed.

## 3. Findings

- **GHSL GHS-POP R2023A** (JRC): epochs 1975–2030; 100 m Mollweide or 3″/30″ WGS84. The 3″
  epoch-2025 product is 10° tiles (12,000² Float64, LZW, 256² internal tiles, `BigTIFF`,
  PixelIsArea, grid origin 79.992° E, 39.100° N). Nepal needs R6_C27 (10 MB zipped) and R7_C27
  (275 MB, dense India). Copyright notice: CC BY 4.0. Census input is CIESIN GPWv4.11,
  disaggregated by GHS-BUILT-S, whose learning set includes OSM buildings, land use, roads and
  rivers (GHSL Data Package 2023 §2.1).
- **WorldPop Global2 R2025A** (University of Southampton): annual 2015–2030, 100 m (3″), per
  country. Nepal 2025 is one file, 32,003,033 B (Float32, LZW, predictor 2, 512² tiles,
  `BigTIFF`, no-data −99,999). Released September 2025 as an alpha that may change; national
  totals match UN WPP 2024. Covariates are described in Woods et al. (2025); building footprints
  from Google and Microsoft; whether OSM roads are among them is **unverified**. CC BY 4.0.
- **Kontur Population**: CC BY, built with OSM: main only (ADR-0030). Not measured.
- Census 2021 (NSO): best for Nepal, blocked on terms and format (Q-011).

## 4. Options compared

Measured 2026-11-09 on the Nepal main build: each grid summed over COD-AB's finest units by
cell centre, against the stated populations of linked records (provinces and districts are
census 2021 except Karnali's, which a record states as 388,713 against the census's
1,688,412).

| Option | Nepal total | Provinces, median / P90 error | Districts, median / P90 error | Time | Licence | Verdict |
|---|---:|---:|---:|---:|---|---|
| WorldPop R2025A, 3″ | 29.63 M | 1.8 % / 2.2 % | 4.5 % / 30.5 % | 17.9 s | CC BY 4.0, OSM inputs unverified | **use** |
| GHSL GHS-POP R2023A E2025, 3″ | 31.63 M | 20.2 % / 52.2 % | 17.6 % / 60.2 % | 42.0 s | CC BY 4.0, OSM in the built-up learning set | reject for Nepal |
| Kontur | – | – | – | – | CC BY, OSM-derived | not measured |

GHSL's provinces: Sudurpashchim +52 %, Karnali −30 %, Gandaki −23 %, Bagmati +20 %.

## 5. What we build ourselves, and why

`forge` streams any `GeoTIFF` grid one internal chunk at a time (`raster::for_each_positive`:
classic or `BigTIFF`, LZW or DEFLATE, Float32 or Float64), so memory is one chunk plus one
number per unit at any extent. Each positive cell counts once, in the finest unit holding its
centre (the last unit hit is tried first); sums go up the parent chain, so levels add up
exactly. The build writes `population-<build>.tsv` (stated, modelled, kept per unit).

## 6. Impact on the plan

D-123; `population:` in a country's `build.yaml`; phase 3 ranking item; attribution (3.7)
gains WorldPop's credit. The pack does not store population yet: it feeds importance only.

## 7. Risks and open items

- WorldPop R2025A is an alpha: pinned by SHA-256; a new release is a deliberate re-pin.
- OSM among WorldPop's covariates decides lite eligibility: read Woods et al. (2025).
- Cell-centre assignment splits border cells to one side: 34,450 people (0.1 %) fell outside
  every unit in Nepal.
- Patan: the municipality of Patan (Baitadi) now outranks Lalitpur city, whose record carries
  the name; the city and Lalitpur Metropolitan City are separate rows (the twin problem). A
  municipality base of 40 or 35 instead of 45 was measured and is worse (0.850, 0.838).
- Census 2021 (Q-011) would replace modelled counts in Nepal when its terms allow.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| WorldPop Global2 | R2025A v1, 2025, NPL constrained 100 m | 2026-11-09 | data.worldpop.org |
| GHSL GHS-POP | R2023A V1-0, E2025, 4326 3ss tiles | 2026-11-09 | jeodpp.jrc.ec.europa.eu |
| `tiff` crate | 0.11.3 (features deflate, lzw) | 2026-11-09 | crates.io |

## 9. Sources

- [WorldPop Global2 R2025A release statement](https://data.worldpop.org/repo/prj/Global_2015_2030/R2025A/doc/Global2_Release_Statement_R2025A_v1.pdf), WorldPop, September 2025.
- [WorldPop Global 2 announcement](https://www.worldpop.org/blog/worldpop-unveils-global-2-next-generation-global-population-dataset/), WorldPop, 2025.
- [WorldPop R2025A in the Earth Engine catalog](https://developers.google.com/earth-engine/datasets/catalog/WorldPop_GP_100m_R2025A_v1_pop), licence CC BY 4.0, accessed 2026-11-09.
- [GHS-POP R2023A dataset](https://data.jrc.ec.europa.eu/dataset/2ff68a52-5b5b-4a22-8f40-c41da8332cfe), JRC, doi:10.2905/2FF68A52-5B5B-4A22-8F40-C41DA8332CFE.
- GHSL Data Package 2023 (light), JRC, in the GHS-POP tile archives, read 2026-11-09.
- [GHSL download index](https://jeodpp.jrc.ec.europa.eu/ftp/jrc-opendata/GHSL/GHS_POP_GLOBE_R2023A/), copyright.txt (CC BY 4.0), accessed 2026-11-09.
- [GHS-POP in the Earth Engine catalog](https://developers.google.com/earth-engine/datasets/catalog/JRC_GHSL_P2023A_GHS_POP), accessed 2026-11-09.
