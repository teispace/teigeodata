---
title: "Census population of Nepal's local levels"
status: accepted
updated: 2026-11-23
owner: maintainer's agents
revisit: 2027-05-23
---

# Census population of Nepal's local levels

## 1. Question

Q-028 asked for the 2021 census count of every local level, to replace the grid's modelled
count (D-123) in the main build. The aim was to rank a populous exact match above a
one-edit match on a smaller unit. Which table, under which terms, and how is it joined to our
units?

## 2. Summary and recommendation

**NSO's municipal table, joined row by row through the CBS crosswalk, main build only** (D-386).

- **The table.** NSO's Central Data Portal, dataset 79, is *Small Area Estimation of Poverty
  2023*. Its sheet "2. Municipal estimates" lists the 753 local levels with their 2021
  households (column F) and population (column G). The rows follow the CBS list's code order.
- **The source.** The workbook is pinned as `nso-np-sae-2023` and mirrored. The portal serves it
  only to a form POST, over http, so builds read it from the mirror and check it by its
  SHA-256 (D-387).
- **The terms.** The portal states no licence, so the source is `proprietary`: only the main
  build reads it, as with the postal codes (D-105).
- **The join.** Each row is joined to its unit by position in the CBS list (`official-names.yaml`,
  D-232), and its Nepali name is checked:
  - 731 rows match exactly, after cutting unit-type words, spaces and punctuation;
  - 20 are near spellings (ङ for ङ्ग, ब for व; at most 34 % apart);
  - 2 are farther, and were checked by hand: Rasuwa's Shuva Kalika is the list's Kalika, and
    Manang's Nason is the list's Nashong. Each has the same district, the same place in the
    order and an English name that agrees. They are listed in `census.yaml`.
- **The wards.** Wards keep their grid shares, scaled to add up to the census count exactly. They
  stay marked modelled.
- **The levels above.** Districts, provinces and the country are re-summed from their units.
- **The finding that changes Q-028.** The census is not the lever for its ranking:
  - The grid's counts were already within a median 2.0 % of the census at this level (P90 4.5 %).
  - The golden set is unchanged by the census, at 437 of 521 hits at 1 on the same local build.
  - मेघौली still gives Melauli (20,658 people, now stated) before Meghauli, because Meghauli the
    settlement has no population at all.

  The next lever is the one Q-028 already named: give a settlement its unit's count, or link a
  municipality and its town.

## 3. Findings

- **Where the counts are.**
  - NSO's data portal, `113.199.192.53:82`, holds downloadable datasets. Its dataset 79
    workbook was downloaded on 2026-11-23: 109,286 B, SHA-256 `8d750770…`.
  - `data.nsonepal.gov.np`, which Q-028 named, did not answer on 2026-11-15.
- **The form.** A GET to `download.php?id=79` gives a 302 and no file; the POST form gives
  the workbook. The portal has no https.
- **Alignment.**
  - The sheet's 753 data rows (rows 3 to 755) are in the same order as CBS's 753 codes.
  - The first exact-name gate refused the table with 22 rows differing. All were spelling
    differences, not shifts: a shifted table differs on almost every row (D-232's district
    file did, 35 of 77).
  - The near-or-checked gate keeps that protection: in the tests, a table shifted by one row
    fails on every row.
- **Effect** (Nepal main, built locally on 2026-11-23, with the census and without it):
  - Local levels: the population before the census differed from it by a median 2.0 %, P90 4.5 %.
  - Wards: 6,748 scaled to their unit's count.
  - Units above: 85 re-summed (77 districts, 7 provinces, the country).
  - The golden gate: identical with and without the census. Both builds are below the checked-in
    baseline, which was set on CI's inputs, and the local recipe lacks some tilesets.
  - The pack is byte-identical across two builds.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Verdict |
|---|---|---|---|---|
| **NSO dataset 79 (SAE 2023 workbook)** | The whole country in one file; CBS code order; the census count, households too | Served by POST over http; no stated licence | None stated: main only | **Chosen** |
| Wikidata `P1082` on local levels | Already linked (D-110) | Mixed years and sources; a linked record can carry another unit's count (D-123) | CC0 | Stays as the stated value where no census reaches |
| Keep the grid (WorldPop) | Open, every level | A model, though within 2 % here | CC BY 4.0 | Stays for lite and for wards |

## 5. What we build ourselves, and why

The forge gains `build/census.rs`:

- a small xlsx reader (zip, shared strings, cells);
- the row-by-row join with its name gate;
- scaling down the levels and summing up them.

It runs after the official names tier. `official::rows` exposes the list's rows with each unit's
key, its list name, a mended name, and its name since a rename. A spreadsheet crate was not
added: the reader is about 120 lines over the zip crate the forge already has, and it is
covered by its own tests.

## 6. Impact on the plan

- D-386 records the stage, D-387 the mirror change.
- Q-028 stays open, with its lever moved from the counts to settlements' populations.
- [Forge](../../04-architecture/forge.md) documents the mirror's releases.

## 7. Risks and open items

- **The terms.** If NSO states a licence that allows it, lite can read the table too.
- **A new census table.** A table in another order needs its own list, or a code column. The gate
  refuses one that doesn't follow its list.
- **Re-pinning is by hand.** The upstream needs a POST.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| NSO dataset 79 workbook | SHA-256 `8d750770…`, 109,286 B | 2026-11-23 | `data/sources.lock`, `nso-np-sae-2023` |
| zip crate (the forge's) | 9.4.1 | 2026-11-23 | `forge/Cargo.lock` |

## 9. Sources

- [NSO Central Data Portal](http://113.199.192.53:82/), dataset 79, *Small Area Estimation of
  Poverty 2023*, accessed 2026-11-23.
- CBS geographical codes, Nepal's 753 local levels (Open Data Nepal), pinned as
  `cbs-np-local-levels`.
