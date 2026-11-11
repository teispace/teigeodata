---
title: "Accuracy labels: how the 301 Nepal places are labelled"
status: research
updated: 2026-11-11
owner: teigeodata maintainers
revisit: 2027-05-11
---

# Accuracy labels: how the 301 Nepal places are labelled

## 1. Question

`tests/golden/np-places.csv` (Q-017, D-062) holds 301 Nepal places drawn by
`forge labels sample`; each needs `label_lat`, `label_lon` at the inhabited centre that carries
the name. The maintainer delegated the labelling to the project's agents (D-209). Which imagery
may a label be drawn on, how is it shown, and how is the agents' labelling kept independent of
what it judges? It feeds `ACCURACY.md` and `forge labels score`.

## 2. Summary and recommendation

- **Imagery: Copernicus Sentinel-2 L2A true colour** (10 m), read as Cloud-Optimized GeoTIFFs
  from the AWS open-data bucket through the Earth Search STAC API, no account. Its terms are
  free, full and open, any use, with the notice "Contains modified Copernicus Sentinel data
  [year]". It is the only imagery found whose terms allow deriving a point at no cost.
- **What the labeller sees** (the plan in [settlement-core](settlement-core.md) §3.4): one chip
  per place, 2 km square (4 km for a town), centred on the middle of the sample's locator cell
  (the source point truncated to 0.01°, about 1 km, so the place lies in the cell north-east of
  the locator; it finds the place but cannot anchor the label), with the raw building points of
  the three footprint sources drawn on top and a 100 m grid (250 m for a town). Nothing from
  the forge's own outputs is drawn (the core, the consensus point, a pack), as `sample`
  requires.
- **How a label is made:** the labeller names the pixel of the centre of the built-up area
  that carries the name: the densest cluster, and the bazaar or crossroads where one is
  visible, preferring the cluster nearest the locator when several are alike. `forge labels set`
  turns the pixel into latitude and longitude from the chip's own transform and writes the row.
  A place with no visible settlement in its chip is left empty with a note.
- **Second pass:** the 60 rows marked `second` are labelled again by a separate agent with no
  access to the first pass, on chips mirrored and shifted so pixel positions do not repeat. The
  agreement (P50, P90 of the distance between passes) is reported with the scores.
- **Honesty in `ACCURACY.md`:** the set is labelled by software agents on 10 m imagery, not by
  people with local knowledge. It says so, with the agreement numbers, and a labelled set made
  by people replaces it when one exists.

## 3. Findings

- Sentinel-2 L2A COGs: true colour `TCI.tif`, 10,980 px square tiles of a 100 km MGRS square,
  UTM (EPSG 326xx), 1,024 px internal tiles, DEFLATE with a horizontal predictor, about 2 MB
  per internal tile (checked 2026-11-11 on `S2C_45RUL_20260121_0_L2A`). A 2 km chip reads one to
  four internal tiles: 2–8 MB per place, nothing stored.
- Earth Search v1 answers anonymous searches by box, date and `eo:cloud_cover`; Kathmandu had
  21 scenes under 5 % cloud between October 2025 and January 2026.
- At 10 m a hamlet of a few houses is a few pixels; the building points make it visible.
- Imagery ruled out: Google and Bing (terms forbid deriving data outside their products), Esri
  World Imagery (tracing permitted for OpenStreetMap only), Maxar Open Data (CC BY-NC, events
  only), EOX Sentinel-2 cloudless after 2016 (CC BY-NC-SA).

## 4. Options compared

| Option | Strengths | Weaknesses | Terms | Verdict |
|---|---|---|---|---|
| a. Sentinel-2 chips with building points, agent labels | open, reproducible, scales to every country | 10 m; no names on the image, so the locator decides between alike clusters | Copernicus free, full and open | **chosen** |
| b. People labelling on a web map | local knowledge, names | needs volunteers; Q-017 waited months | as a | the later replacement |
| c. A rule (densest building cell near the locator) | instant | is the core's own method: the score would judge the core by itself | n/a | rejected (D-060 rule) |

## 5. What we build ourselves, and why

`forge labels chips <cc> <out-dir>` (fetch each chip by HTTP range from the scene with the least
cloud in the dry season, decode the covering internal tiles with the `tiff` crate, draw the
building points and the grid, write a PNG and the chip's transform) and
`forge labels set <cc> <picks.tsv>` (pixel → latitude and longitude, written into the CSV with
`labeller` and the pass). No imaging library is needed: the PNG writer is zlib plus CRC-32.

## 6. Impact on the plan

- Phase 3 §3.7: `ACCURACY.md` follows the scores on this set.
- Q-017 is answered by this method; the people-made set stays an idea.

## 7. Risks and open items

- Two alike clusters within the locator's kilometre: the label may take the wrong one; the
  second pass and the notes show where.
- Clouds and terrain shadow in the hills: the scene choice prefers October to January.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Earth Search STAC | v1 | 2026-11-11 | earth-search.aws.element84.com/v1 |
| Sentinel-2 L2A COGs | collection `sentinel-2-l2a` | 2026-11-11 | AWS open data registry |

## 9. Sources

- [Sentinel-2 Cloud-Optimized GeoTIFFs](https://registry.opendata.aws/sentinel-2-l2a-cogs/), AWS open data registry, accessed 2026-11-11.
- [Legal notice on the use of Copernicus Sentinel data](https://sentinels.copernicus.eu/documents/247904/690755/Sentinel_Data_Legal_Notice), accessed 2026-11-11.
- [Earth Search](https://github.com/Element84/earth-search), Element 84, accessed 2026-11-11.
