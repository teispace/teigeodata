---
title: "Population as a field of every place"
status: accepted
updated: 2026-11-15
owner: format, forge, engine
revisit: 2027-05-01
---

# Population as a field of every place

## 1. Question

The vision and the [schema](../../05-data/schema.md#population-and-other-optional-data) promise
each place's population, and the main build computes one for every admin unit (D-123) and
reads one for settlements (GeoNames, Wikidata, OSM). On 2026-11-15 no pack stored it and no API
returned it: the forge used the counts only for search importance (D-266), and COMPONENTS, the
section reserved for optional per-place fields (D-076), was neither written nor read. How do
packs carry a population, and what does every API return?

## 2. Summary and recommendation

- **Store it in COMPONENTS as component 1, `population`** (D-351): field 1 the number of people,
  field 2 where it came from (`stated` 1, `modelled` 2). Format 1.0 already defines COMPONENTS
  (column `component << 8 | field`, presence as sorted rows or a bitmap, B6), so this is data in
  the frozen format, not a format change: a format-1.0 reader opens it.
- **Write it for every current place with a count above 0**: admin units (the stated count kept
  when the population grid agrees within 1.5×, else the modelled one, D-123), settlements and
  others with a source's count, and the world part's countries, units and cities (Wikidata and
  GeoNames, all stated). Former units carry none: their count is of a unit that no longer
  exists, and nothing records its year.
- **No year.** Of the sources, only Wikidata states a count's date (P585 on P1082); GeoNames and
  OSM mostly do not. A year on a few rows and none on most would read as precision the data
  does not have. The source field says what a user most needs: a count someone stated, or a
  model's.
- **Engine:** `Engine::population(&Place) -> Option<Population { people, source }>`, feature
  `extra` (the install component COMPONENTS belongs to), on by default. A layer installed
  without `extra` answers `MissingComponent`, as every other component does.
- **API and bindings:** two nullable columns in every place result, `population` (`u32`) and
  `population_kind` (dictionary). Every binding reads columns by name, so no binding code
  changes; the parity fixture carries populations and every language checks the columns.
- **Merges keep it:** `extract::merge` (the shipped baseline, `tgd extract`) carries every
  component of COMPONENTS for the kept rows, field by field, with no per-row allocation.
- **Gate G-pop:** every build reads COMPONENTS back and compares each row's count and source
  with what it wrote; the summary reports how many are stated and modelled, and the bytes.

## 3. Findings

- The forge's `Place.population` is set by cluster merging (the largest count of a cluster's
  records), admin links (D-110), the grid (D-123), corrections (`set-population`, D-117) and the
  world part's Wikidata snapshot (D-266). The grid's report says which units it filled or
  replaced (`population-<build>.tsv`); a correction applied later makes the count stated again.
- Nepal's main build (data-build run 43719616814, 2026-11-15): 757 units took a modelled
  population, 19 stated ones were replaced; level 3 has 23 stated counts, median error of the
  model against them 81% (`np-summary-main.md`). The source field is what tells a user that a
  local level's count is modelled.
- Bit-packed with frame of reference, a count costs at most 31 bits per place with one, a source
  1 bit, and presence the smaller of a bitmap (1 bit per place) or sorted rows (B6).
  Measured sizes are in §8 once the CI build has run.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| COMPONENTS component 1 (chosen) | In format 1.0 already; sparse presence; one reader for future optional fields | none found | **chosen** |
| A new section kind | Its own codec | A format minor, a new component, a new reader | rejected: COMPONENTS exists for this |
| Recover counts from SEARCH importance | No new data | Lossy (5 per tenfold), mixed with rank and level | rejected |

## 5. What we build ourselves, and why

The typed view (`components::Populations`), its writer (`encode_populations`), the generic
COMPONENTS merge in `extract`, the forge's `populations` and G-pop. Proved: the format round
trip in every presence form, the engine read and its missing component, a slice and a merge
that keep each row's count, the forge's choice of rows and source, and G-pop failing on a
changed or missing count (tests named in the change).

## 6. Impact on the plan

- [Schema](../../05-data/schema.md#population-and-other-optional-data): `population` and
  `population_kind`; no `population_year` in v1.
- [Pack format](../../04-architecture/pack-format.md#components): component 1 assigned.
- Phase 8: the data release carries populations (rebuilt in CI).

## 7. Risks and open items

- Level-3 counts in Nepal's main build are modelled, with a large error at that level
  ([Q-028](../../07-planning/open-questions.md)): the source field says so, and census counts
  replace them once an openly licensed table is pinned.
- The lite build has no grid: its units carry the stated counts only.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Pack format | 1.0 (D-336) | 2026-11-15 | `crates/teigeodata-format` |

## 9. Sources

- [Presence of optional fields (B6)](presence.md)
- [Population grids for admin units](population-grids.md)
- Wikidata property P1082 (population) and its qualifier P585 (point in time),
  <https://www.wikidata.org/wiki/Property:P1082>, read 2026-11-15
