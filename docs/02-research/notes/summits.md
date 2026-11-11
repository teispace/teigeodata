---
title: "Summits and their surveyed heights"
status: accepted
updated: 2026-11-11
owner: teigeodata maintainers
revisit: 2027-05-11
---

# Summits and their surveyed heights

## 1. Question

Plan step 3.4 "Surveyed heights for summits" (D-089): a DEM under-reads sharp summits (GLO-30
reads Everest 111 m low, [DEM note](dem-section.md) §7), so a summit's height should be its
surveyed one. Which places are summits, where do their heights come from, and how is a wrong
stated height kept out? It feeds D-201.

## 2. Summary and recommendation

- **Summits become places.** Until now a pack held settlements, localities, admin units and
  timezone areas only: no summit had a height to correct. Point features with a height of
  their own become `natural` places: OSM nodes `natural=peak|volcano|hill|saddle` and
  `mountain_pass=yes` (main build), GeoNames `T.PK`, `T.MT`, `T.HLL`, `T.VLC`, `T.PASS`,
  `T.SDL` (both builds). Ranges, ridges, valleys and spurs are lines or areas, not places.
- **Height rule** ([timezone and elevation note](timezone-and-elevation.md) §5.B.4): a natural
  place takes the median of the heights its records state where that is within
  **50 m + 1 %** of the DEM at its point or of the DEM's highest sample within 3″ (about
  90 m); otherwise it keeps the DEM's height and the summary lists it as refused. A pass sits
  at a saddle, so the point's own height is the reference that admits it; a summit's
  reference is the nearby maximum.
- **No new format bit.** The forge's read-back check, which holds every other row to the DEM
  within 1 m, holds a natural place to the same 50 m + 1 % rule, recomputed from the pack
  alone. A client wanting the terrain height asks `elevation_at(place.point)`.
- **Not moved.** The note's snap of a summit to the DEM's local maximum is not built: the
  point accuracy comes first, and a DSM's maximum is not the summit's position. **Open**.

## 2b. As built (2026-11-11, Nepal main)

- 2,726 natural places with a height; 789 with a stated one; 672 taken, 117 refused; 125 more
  than 200 m outside every unit left out (Tibet and India).
- G-PEAK, stated minus the DEM's highest sample within 90 m, on the 672: P10 −34 m, P50 15 m,
  P90 82 m. Everest stores 8,849 m.
- The largest refusals are points far from their summit (a stated 8,410 m where the DEM reads
  7,897 m): the snap to the local maximum, still open, would admit them.
- The golden gate did not lose a hit to the new names.
- Lite (GeoNames and Wikidata only, NASADEM): 965 natural places, 127 with a stated height,
  91 taken, 36 refused; G-PEAK P10 −25 m, P50 40 m, P90 74 m. The golden gate is unchanged
  class by class; substring 9 of 9.

## 3. Findings

- GeoNames `NP.txt` (pinned 2026-11): 215 `T.PK` (27 with an elevation), 528 `T.MT` (47),
  26 `T.HLL`, 181 `T.PASS` (3), 16 `T.SDL`; 850 `T.MTS` (ranges) and 633 `T.RDGE` (ridges)
  are left out.
- OSM: about 9,100 Nepal objects carry `ele`, mostly summits and passes, with mixed provenance
  ([Nepal sources](nepal-sources.md) §5). OSM's `ele` is metres above mean sea level by the
  wiki's definition; our reader already parses `m`, `ft` and thousands separators.
- Everest: 8,848.86 m (Nepal and China, announced 2020-12-08); GLO-30's highest sample on
  tile N27 E086 is 8,737.8 m, 111 m low, within the 50 m + 1 % (138 m) tolerance.
- No open official list of Nepal's peak heights was found; the permit-peak tables have no
  stated licence. Survey of Nepal trig points are not open.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Verdict |
|---|---|---|---|---|
| a. Stated height when it agrees with the DEM (50 m + 1 %) | uses every source; a gross error (feet, a wrong peak) is refused | a stated height wrong by less than the tolerance passes | OSM ODbL (main), GeoNames CC BY (both) | **chosen** |
| b. DEM only | one source, consistent | Everest 111 m low; every sharp summit low | n/a | rejected: the reason for this step |
| c. Stated height always | simplest | a feet value or a copy from a nearby summit goes in unchecked | as a | rejected |

## 5. What we build ourselves, and why

The rule above, in `forge/crates/forge/src/build/elevation.rs` (`summits`), with its
read-back check, and the readers' new `natural` kind. The summary's line reports how many
natural places took a stated height, how many were refused (the ten largest listed), and the
G-PEAK measure: stated minus the DEM's nearby maximum, P10, P50 and P90.

## 6. Impact on the plan

- D-201 in the decision log; phase 3 §3.4 "Surveyed heights for summits" closes with the Nepal
  numbers.
- The schema's `natural` kind is now produced (it was reserved).

## 7. Risks and open items

- Search: thousands of new names. A summit named like a settlement may outrank it; the golden
  gate measures it.
- Snap to the local maximum: open, as above.
- Wikidata summits (P2044 elevation): the pinned query does not ask for mountains; a later
  re-pin can add them (CC0, lite too).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| GeoNames NP.txt | pinned in `data/sources.lock` (`geonames-np`) | 2026-11-11 | local store |
| Geofabrik Nepal | 2026-11-05 extract (`geofabrik-np`) | 2026-11-11 | local store |
| Copernicus GLO-30 | AWS COG (`copernicus-glo30-np`) | 2026-11-11 | local store |

## 9. Sources

- [Key:ele](https://wiki.openstreetmap.org/wiki/Key:ele), OpenStreetMap wiki, accessed 2026-11-11.
- [Tag:natural=peak](https://wiki.openstreetmap.org/wiki/Tag:natural%3Dpeak), OpenStreetMap wiki, accessed 2026-11-11.
- [GeoNames feature codes](https://www.geonames.org/export/codes.html), accessed 2026-11-11.
- Everest's height, Nepal and China joint announcement, 2020-12-08 (as cited in the
  [timezone and elevation note](timezone-and-elevation.md)).
