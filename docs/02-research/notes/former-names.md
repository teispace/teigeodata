---
title: "Former and everyday names of world places"
status: accepted
updated: 2026-11-14
owner: forge
revisit: 2027-05-14
---

# Former and everyday names of world places

## 1. Question

A search by a former or everyday name (Poona, Simla, Tanjore, Peking, Ceylon) should find the
place. D-265 gave cities their GeoNames alternate names in the layer's languages; which source
covers the rest, world-wide, at what cost in bytes and noise? Feeds `forge world` (D-268) and
the substring score (D-269).

## 2. Summary and recommendation

Decided (D-268): Wikidata's aliases in the layer's language, on every country, unit and city,
from a pinned CC0 snapshot; an alias is kept only when it adds a search key. Decided (D-269):
a substring match of 3 or 4 code points weighs less than one of 5 or more.

## 3. Findings

### 3.1 What D-265 already finds (2026-11-14)

On the profile layer: Benares, Bombay, Cawnpore, Allahabad, Bangalore, Gurgaon, Calicut,
Pondicherry, Rangoon, Saigon and Leningrad found their place. Banaras, Baroda, Poona, Simla,
Cochin, Jubbulpore, Tanjore, Peking and every former country name (Ceylon, Burma, Siam) did
not: GeoNames files them with **no language** (Baroda, Poona, Simla, Tanjore, Jubbulpore,
Cochin), and D-265 reads cities only.

### 3.2 GeoNames' language-less names

On the cities of 5,000: 106,926 language-less names, 67,412 Latin ones not equal to the
city's name, 64,280 after folding as search folds (753 kB raw), up to 16 per city at the long
tail, among them other places' names (Tamil Nadu's spellings filed under Chennai) and
romanisation variants. Too noisy to take whole, and still cities only.

### 3.3 Wikidata's aliases

`skos:altLabel` in English of every item with a GeoNames ID and a population, via QLever: the
full IRI on every row timed the export out after about 7.7 MB; a bare `Q…` string, and the
query dropping what is no name (under 4 characters, starting in lower case such as `th` or
`lk`, inverted with a comma such as `Lanka, Sri`), gives 126,639 rows in 4.58 MB in one
response, equal row for row to the same query fetched in nine parts. It holds Banaras, Poona,
Simla, Cochin, Jubbulpore, Tanjore, Calcutta, Trivandrum, Gauhati, Formosa and Ceylon (on
the Dominion's item, which shares Sri Lanka's GeoNames ID), and also noise: qualified names
(`Tokyo (Japan)`), the name with words around it (`State of Japan`, `Tokyo Prefecture`),
another place's name.

## 4. Options compared

Gates: the world set (60 cases, then 72 with 12 former names), India's 44, the layer's 83,
Nepal's 521; layer wire bytes.

| Variant | Aliases | Layer wire | World | Layer golden | Verdict |
|---|--:|--:|---|---|---|
| D-265 only | 0 | 9,471,607 B | 64 / 72 | 83 | the gap |
| GeoNames language-less, folded | 64,280 | not built (753 kB raw) | | | too noisy |
| Wikidata aliases, folded dedupe | 18,762 | 9,915,206 B | 59 / 60 | 82 (`japa` → Tokyo via "Tokyo (Japan)") | noise |
| + one row per item (city first), no `(`, none holding the place's name | 12,436 | 9,738,489 B | 59 / 60 (`Var`) | 83 | `Var` left |
| **+ substring weight by length (D-269)** | 12,436 | 9,738,489 B | **72 / 72** | 83 | chosen |

`Var` lost Varanasi to India: India's alias "Bharatvarsh" holds "var" inside a word, and a
substring match (20) on a country of 1.4 billion (importance 90) beat a prefix match (30) on a
city of 1.2 million. The data cannot drop it on a general rule (India has no Latin "Bharat"
name), and the substring index is derived on the device from every name, so a filter by name
kind would change derived bytes older engines check. A 3-letter run inside a word is weak
evidence: the substring bonus becomes 10 at 3 code points, 15 at 4, 20 from 5. Nepal's gate is
unchanged (recall@1 0.848, substring class 1.000), India 44, layer 83.

## 5. What we build ourselves, and why

`world_aliases` (the snapshot's reader and the keep rules), the profile field `world_aliases`,
and the engine's `substring_bonus`.

## 6. Impact on the plan

D-268, D-269; the world golden set gains 12 former names; the next data release.

## 7. Risks and open items

- Wikidata aliases are edited by anyone; a wrong one finds a wrong place, and the snapshot is
  pinned and re-read on a schedule like every source.
- Former names in other languages (Nepali, Hindi aliases) are the same query with another
  `LANG`; added when a profile needs them.
- Burma, Siam and Persia are filed under the countries only as longer forms ("Union of Burma",
  "Kingdom of Siam"), found from a word inside; Q-027 covers weighing names by language.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Wikidata via QLever | snapshot of 2026-11-14 | 2026-11-14 | `wikidata-aliases-en` in `data/sources.lock` |
| GeoNames alternate names | `alternateNamesV2` of 2026-11-06 | 2026-11-14 | `geonames-alternate-names` |

## 9. Sources

- [Wikidata query service on QLever](https://qlever.dev/wikidata), queried 2026-11-14 (`forge/queries/wikidata-aliases.rq`).
- [GeoNames export readme](https://download.geonames.org/export/dump/readme.txt), the `alternateNamesV2` columns, accessed 2026-11-14.
- [Wikidata: Aliases](https://www.wikidata.org/wiki/Help:Aliases), what an alias is for, accessed 2026-11-14.
