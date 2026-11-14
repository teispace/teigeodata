---
title: "One importance scale for countries, regions and cities"
status: accepted
updated: 2026-11-14
owner: forge
revisit: 2027-05-14
---

# One importance scale for countries, regions and cities

## 1. Question

Q-026: why does `Var` put Varanasi (1.2 million people) 20th, behind Var, Varese and Vårdö
(450 people)? More generally: does search rank a big city fairly against a country or a
first-level unit, world-wide? Feeds `pack::importance` (D-052, D-124, D-158) and the world
part's data (D-266).

## 2. Summary and recommendation

The importance of countries and first-level units sat on a different scale from cities': a
country 100 and a first-level unit 90 before population, a city 44 plus 5 per tenfold
population, and world first-level units had no population at all. Match bonuses span 20
(substring) to 60 (exact), so kind beat match quality: every first-level unit in the world
outranked every city in the prefix tier, and a country matched loosely by another language's
name outranked a big city matched exactly.

Decided (D-266), measured on three golden sets:

- A country weighs 45 and a first-, second- or third-level unit 45 before population, on the
  cities' scale of 5 per tenfold population; a first- and second-level unit without a count
  takes a typical one (1,000,000 and 200,000). Below them D-124's calibration stays.
- World units take their population from Wikidata (a pinned CC0 snapshot of every country's
  first- and second-level units, best-rank statements, the latest dated value).
- A settlement of 1,000,000 people or more ranks as a city whatever its source class.
- A place inside a country that shares the country's name ranks just below it.
- A city outside every unit's simplified outline is placed in the nearest unit of its country
  within 10 km.

## 3. Findings

### 3.1 How the major geocoders weigh places (checked 2026-11-14)

- **Nominatim**: importance is primarily from Wikipedia (link counts); a place without an
  article takes a fallback from its search rank, `0.40001 − rank_search / 75` since February
  2024 (before: `0.75 − rank_search / 40`). Kind sets only the fallback. Sources:
  [Ranking](https://nominatim.org/release-docs/develop/customize/Ranking/),
  [Importance](https://nominatim.org/release-docs/4.2/customize/Importance).
- **Pelias**: a `function_score` boost from `log1p(population)` and `log1p(popularity)`, each
  capped at 20, for every layer including regions; focus and layer boosts are bounded so they
  approach but do not override a popular place
  ([pelias/api #729](https://github.com/pelias/api/pull/729), the `pelias-sorting` docs).

Both weigh a region by its people or popularity, not by its level alone, and keep the boosts
small next to text match. Ours did the opposite.

### 3.2 Our scale before (measured 2026-11-13/14)

| Place | Kind | Importance | Why |
|---|---|--:|---|
| Var | first-level unit, no population | 90 | base 90 |
| Vårdö (450 people) | first-level unit, no population | 90 | base 90 |
| Varanasi (1.2 M) | city, rank 3 | 74 | 30 + 14 + 5·log10(1.2 M) |
| India (1.4 B) | country | 145 | 100 + 45 |

`Istan` gave India (via its Turkish name Hindistan, substring 20 + 145) before Istanbul
(prefix 30 + 80); `Tor` gave Turkey (another language's name) before Toronto.

### 3.3 Population for units

World first-level units (Natural Earth) carry no population. Wikidata via QLever: every
country (Q6256) `P150` one or two steps, best-rank `P1082` with `P585`: 32,419 items,
62,692 rows, 4.97 MB (2026-11-13). Of the world part's 4,571 first-level units 3,521 take a
count (77%); of India's 762 districts 141 (Wikidata files most under divisions). Without the
counts the new scale gave 42 of 44 on the world set (Vårdö back above Varanasi, Kolkata above
California for `Cal`); with them 44 of 44.

## 4. Options compared

Golden sets: Nepal's 521 cases (pack, gate per class), India's 44 and the world's 60 (world
part). The world set was written for this question: exact names and prefixes of big cities,
regions and countries, ordered as the major geocoders order them, IDs checked by name,
country and kind.

| Variant | World | India | Nepal r@1 | Nepal gate | Verdict |
|---|--:|--:|--:|---|---|
| Before (100 / 90 / 70, no unit counts) | 32 / 44 | 44 | 0.839 | ok | the problem |
| V1: all kinds on one scale, small bonuses, default counts for every kind | 39 / 44 | 44 | 0.852 | **fails** (substring 1.000 → 0.556, native exact 0.929 → 0.857) | villages lost to defaults |
| V2: only the top rescaled (country 35, units 45), D-124 below | 44 / 44 | 44 | 0.848 | ok | countries now too low |
| V2, city from 100,000 people | 44 / 44 | 44 | 0.845 | **fails** | 1,000,000 kept |
| V2, country 35 / 45 / 55 (60-case set) | 52 / 58 / 58 | 44 | 0.848 | ok | 55 loses `Istan`, `Tor` |
| **V2, country 45, country-name rule** | see §5 | 44 | 0.848 | ok | chosen |

Not taken: Wikipedia- or sitelink-based prominence (Nominatim's way): a better signal of
fame, but a second data dependency and a re-calibration of every class; revisit if the
population scale proves too coarse. Rescaling the match bonuses instead: changes every
query class at once and was not needed.

## 5. What we build ourselves, and why

`pack::importance` (forge), `pack::rank_of`, `pack::below_their_country`, and
`world_population` (the snapshot's reader). The final measurement is in the decision log
(D-266) and the progress page.

## 6. Impact on the plan

D-266 in the decision log; Q-026 closed; the world golden set `tests/golden/world-search.tsv`;
the forge page's world-part flags (`--populations`); the next data release.

## 7. Risks and open items

- Q-027: a name in a language other than the query's weighs as much as one in it (`Istan`,
  `Tor`); weighting names by the search languages is its own question.
- Unit counts come from Wikidata's best-rank statements, of varying years; a unit without one
  takes its level's typical count.
- India's districts mostly lack a count; the city-first rule covers the common case.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Wikidata via QLever | snapshot of 2026-11-13 | 2026-11-13 | `wikidata-unit-population` in `data/sources.lock` |
| Nominatim ranking docs | develop (5.x) | 2026-11-14 | nominatim.org |
| Pelias API | master | 2026-11-14 | github.com/pelias/api |

## 9. Sources

- [Place Ranking in Nominatim](https://nominatim.org/release-docs/develop/customize/Ranking/), Nominatim project, accessed 2026-11-14.
- [Importance](https://nominatim.org/release-docs/4.2/customize/Importance), Nominatim project, accessed 2026-11-14.
- Nominatim commit "reduce importance when computed from search rank" (February 2024), git.openstreetmap.org/nominatim.git, accessed 2026-11-14.
- [pelias/api pull request 729](https://github.com/pelias/api/pull/729), the autocomplete focus and population boosts, accessed 2026-11-14.
- [Wikidata query service on QLever](https://qlever.dev/wikidata), queried 2026-11-13 (`forge/queries/wikidata-unit-population.rq`).
