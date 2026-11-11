---
title: "Sizes: where a pack's bytes go"
status: living
updated: 2026-11-11
---

# Sizes: where a pack's bytes go

Every consumer plans its install from this page (TG-11). Numbers come from
`cargo xtask pack-sizes <pack.tgd> --langs ne,en`, measured on 2026-11-11 on the release
`2026.11.0` builds. **Disk** is the bytes on disk: a pack is read in place, so disk is the
section's stored bytes. **Wire** is the transport asset (zstd 19, frames cut on the pack's
structure, [download and delta](../02-research/notes/download-and-delta.md)); the gate G-wire
(`check-pack`) proves the parts add up to the asset and enforces each country's budgets. The
command also prints every column of every section, which is where to look before optimising.

## Components

Wire bytes per component, with bytes per place in brackets.

| Component | Nepal lite (95,726 places) | Nepal main (102,367) | World part, lite (39,395) |
|---|--:|--:|--:|
| point | 498,652 (5.2) | 533,377 (5.2) | 319,330 (8.1) |
| elevation (stored heights) | 124,234 (1.3) | 132,974 (1.3) | 50,599 (1.3) |
| admin (hierarchy, ISO codes) | 114,136 (1.2) | 120,401 (1.2) | 96,373 (2.4) |
| names | 745,671 (7.8) | 937,099 (9.2) | 369,063 (9.4) |
| search | 1,312,683 (13.7) | 1,650,534 (16.1) | 533,260 (13.5) |
| substring | 1,820,323 (19.0) | 2,079,900 (20.3) | not built |
| timezone | 277 | 607 | 9,514 |
| timezone-index (zone outlines) | 368,538 | 1,605,839 | in boundaries |
| tz-rules | 248 | 662 | not built |
| boundaries (outlines) | 2,573,601 (26.9) | 2,573,601 (25.1) | 2,186,041 (55.5) |
| admin-index (point → unit) | 719,289 (7.5) | 719,289 (7.0) | 920,345 (23.4) |
| postal | not in lite (D-205) | 7,285 | not built |
| dem (30 m) | 68,384,347 | 64,167,225 | not built |
| dem-coarse (15″, about 460 m; D-225) | 517,997 | not measured | not built |
| head, seek table, block hashes | 155,391 | 153,910 | 8,072 |
| **asset** | **76,817,390** | **74,682,703** | **4,492,597** |
| asset without the DEM | 8,433,043 | 10,515,478 | 4,492,597 |

The world part's outlines are its countries and first-level units (Natural Earth, 248 and
4,578 polygons).

## Names by script

What share of places carries at least one name in each script.

| Script | Nepal lite | Nepal main | World part, lite |
|---|--:|--:|--:|
| Latin | 100.0% | 99.7% | 100.0% |
| Devanagari | 3.3% | 9.1% | 0.7% |

Most records carry no language tag (`und`: 112,426 of 126,456 in Nepal lite), so the script is
the honest measure. **Nepali names are the largest gap** against
[Tithipatra's addendum](../01-product/consumers/tithipatra.md) (§2.1: a Nepali and an English
name for every place): 9 in 10 of Nepal's places and nearly every world city lack one.

Cutting NAMES to each place's canonical name plus Nepali and English:

| Pack | NAMES now (wire) | Cut to canonical + ne + en (wire) |
|---|--:|--:|
| Nepal lite | 744,394 | 453,841 (10.8 B/place on disk) |
| Nepal main | 928,020 | 504,979 |
| World part, lite | 369,063 | 250,827 |

## Outlines by admin level and level of detail (Nepal)

Each arc counted once, at the coarsest level using it. Bytes are the ARCS section's wire bytes
shared by vertex count, an estimate.

| Down to | Polygons | Vertices | Wire (est.) |
|---|--:|--:|--:|
| country | 1 | 65,786 | 272,531 |
| province | 7 | 113,875 | 471,749 |
| district | 77 | 239,232 | 991,064 |
| local level | 775 | 593,610 | 2,459,143 |

| Level of detail | Tolerance | Vertices kept |
|--:|--:|--:|
| 0 | full | 100% |
| 1 | 5 m | 51.0% |
| 2 | 20 m | 21.4% |
| 3 | 100 m | 7.3% |
| 4 | 500 m | 2.2% |

## What this means for an embedded layer

Today, Nepal lite without the DEM plus the world part is 12.9 MB on the wire, before the world's
cities of 5,000, India's towns and the missing Nepali names. Four parts are 87% of it:
outlines (4.8 MB), the substring and search indexes (3.7 MB), the point-to-unit indexes
(1.6 MB) and names (1.1 MB). Where each goes is planned in
[embedded layer](../02-research/notes/embedded-layer.md).

## Embedded layers built from profiles

`forge embed --profile <name>` (D-214), lite, 2026-11-11. Per component, `tithipatra`'s wire bytes: point 1,035,391; elevation 229,894; admin 251,730; names 1,103,972; search 2,111,132; timezone 13,564; tz-rules 63,568; boundaries 2,776,845; admin-index 1,146,916.


| Profile | Rows | Disk | Wire |
|---|--:|--:|--:|
| `baseline` (world part and Nepal's slice; today's file, byte for byte) | 39,395 | 5,891,068 | 4,492,597 |
| `tithipatra` (world part with every city of 5,000, all of Nepal at 20 m; names in Devanagari Nepali, Hindi where Wikidata has no Nepali, and English, Nepal's gaps filled by the transliterator; zone rules for all 376 zones; the coarse DEM) | 170,892 | 15,881,364 | 10,302,696 |
| `points-en` (points, heights, admin, English names and search; no outlines) | 39,395 | 1,822,396 | 1,172,779 |

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-11 | `tithipatra` with zone rules for its 376 zones: +63.6 kB wire | D-224 |
| 2026-11-11 | `tithipatra` with Wikidata `ne`/`hi` labels on 13,724 world places: 8.69 MB wire (+0.41 MB) | D-221 |
| 2026-11-12 | `tithipatra` with the coarse DEM (D-225) and transliterated Nepali names for 93,758 Nepal places (D-228): 10.30 MB wire (dem-coarse 518 kB, names 1.61 MB, search 2.63 MB) | D-228 |
| 2026-11-11 | `tithipatra` with cities of 5,000 (69,972) and `ne-Deva`: 8.28 MB wire, 35,825 more rows for 0.96 MB | TG-13, D-219 |
| 2026-11-11 | Embedded layers from profiles | D-214 |
| 2026-11-11 | First tables: components, names by script, outlines by level and level of detail | TG-11 |
