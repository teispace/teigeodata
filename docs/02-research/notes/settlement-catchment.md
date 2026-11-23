---
title: "Ranking a settlement without a count by its catchment"
status: accepted
updated: 2026-11-23
owner: maintainer's agents
revisit: 2027-05-23
---

# Ranking a settlement without a count by its catchment

## 1. Question

Q-028's lever after the census (D-386): Meghauli the settlement has no population, so the
exact query मेघौली ranks it one point below Melauli, a one-edit match on a local level of
20,658 (90 against 91). Would giving every settlement without a count the people the grid
puts around it, for ranking only, fix such cases without losing elsewhere?

## 2. Summary and recommendation

**Rejected** (D-392). Both variants fix Meghauli and lose on Nepal main's golden set:

| Variant | hits@1 of 521 | recall@1 |
|---|--:|--:|
| A: today | 437 | 0.839 |
| B: the catchment as a population (5 points per tenfold) | 426 | 0.818 |
| C: B capped at 5 points | 433 | 0.831 |

- B lifts villages over the units of their name: prefix 0.750 → 0.600, historical
  0.841 → 0.750, variant and ambiguous worse. This is the lesson `RANK_BONUS` already records
  (D-124).
- C loses four historical queries and gains one colloquial. Each lost query names a former VDC
  by its joined spelling ("SyauleBazar"): the former unit matches exactly (60 + `FORMER` 25 =
  85), the settlement of the same place loosely (55 + 30 = 85). Today these are exact ties won
  by ID; any catchment of 2 points or more flips them, and Meghauli needs at least 2.

No settlement-wide lift works: the scores of a former unit and of today's settlement sit
exactly 5 apart in the very cases a lift must not touch. The code was not kept.

## 3. Findings

- **The catchment.** Each populated cell of the grid counts for the nearest settlement within
  2 km, east-west distance scaled by latitude, with settlements bucketed on a 2 km grid:
  one pass of the population stage, one entry per settlement.
  - Nepal main: 91,938 settlements got one, median 136 people, P90 729, covering 99.9 % of
    the grid's people.
  - The build took 5 min 43 s, as before.
- **The score gap.** An exact match gets 60, a one-edit match 25. Melauli is
  45 + 5·log10(20,658) = 66, so 91; Meghauli is 30, so 90.
- **The ties.** The four historical queries were exact ties at 85 before the change, so
  today's result for them is fragile too.

## 4. Options compared

| Option | Effect | Verdict |
|---|---|---|
| Catchment as a population (B) | Meghauli first; −11 hits@1 | Rejected |
| Catchment capped at 5 (C) | Meghauli first; −4 hits@1 (historical ties) | Rejected |
| Raise `FORMER` above a settlement | Keeps the ties but makes a former unit beat today's place of its name | Rejected: a search means today's place (D-110) |
| Link a settlement to the unit it names (Q-028's other lever) | Narrow: only places sharing a unit's name | Open |

How others rank places with no count: Nominatim derives importance from Wikipedia and, without
an article, from the place's rank, finding population too thin world-wide to use. GRID3 puts
gridded population on settlement extents, as operational estimates rather than official counts.

## 5. What we build ourselves, and why

Nothing kept.

## 6. Impact on the plan

Q-028 stays open. Its remaining levers are a link from a settlement to the unit it names, and
breaking the former-unit ties on purpose rather than by ID.

## 7. Risks and open items

- The four historical queries hang on exact ties broken by ID.
- The local build scores below the checked-in baseline (437 against 451), as noted for D-386;
  this A/B compares builds on the same machine.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| WorldPop grid (the forge's) | Global2 R2025A | 2026-11-23 | `data/sources.lock` |
| Nominatim importance docs | 5.1 | 2026-11-23 | nominatim.org |

## 9. Sources

- [Nominatim: Importance](https://nominatim.org/release-docs/latest/customize/Importance/),
  accessed 2026-11-23.
- [New Wikimedia-based scoring file available](https://nominatim.org/2024/08/07/wikimedia-file.html),
  Nominatim, 2024-08-07.
- [GRID3 SSA population v1.0 README](https://data.worldpop.org/repo/wopr/_MULT/population/GRID3_SSA2025/GRID3_SSA_population_v1_0_README.pdf),
  WorldPop, accessed 2026-11-23.
