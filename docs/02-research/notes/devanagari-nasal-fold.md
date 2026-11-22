---
title: "Devanagari nasal clusters in the search fold"
status: accepted
updated: 2026-11-22
owner: agents
revisit: 2027-05-22
---

# Devanagari nasal clusters in the search fold

## 1. Question

Q-037 (found by TG-28, 2026-11-17): `लन्डन` misses London, stored `लण्डन` (Hindi `लंदन`).
Which Devanagari spelling variants should search fold, and how does a fold change after 1.0
keep packs and engines of different releases meeting?

## 2. Summary and recommendation

- **Fold Devanagari's dental न with a virama before a stop of any class to the anusvara.**
  Nepali and Hindi write a loanword's n so (काउन्टी = काउंटी, लन्डन = लंडन); the 1.0 fold
  (D-220) took a class nasal only before a stop of its own class, so ण्ड folded and न्ड did not.
- **Not म before another class's stop** (नाम्चे, इम्जा, सिम्जुङ: a real m in Tibeto-Burman
  names), **not other scripts** (Bengali writes ন্ট in loanwords and its ং is ng), **not ड/द**
  (लंदन against लंडन is another letter; Hindi's London is found by its own Hindi name, which
  the data carries).
- **Both folds meet.** Since 1.0 a fold change splits index and query: the forge also writes
  each such name's key under the 1.0 fold (`fold_1_0`), and the engine also looks a query up
  under it, exact and loose, when it differs. New engine on an old pack, old engine on a new
  pack: each finds at least what 1.0 found.

## 3. Findings

- Wikidata's Nepali and Hindi labels (the pinned `wikidata-labels-ne`, `wikidata-labels-hi`):
  of 11,810 Nepali labels, 440 hold a nasal with a virama before a stop of another class; of
  68,252 Hindi labels, 978. In Nepali the dental न leads (न्ट 162, न्ड 90, न्ज 42, न्क 21);
  म before another class (म्ज 15, म्द 14, म्च 12) is native names with a real m.
- Over the 73,948 distinct Devanagari labels, the new rule merges 37 groups that were apart
  under the 1.0 fold. All 37 are spellings of one name (ओन्टारियो/ओंटारियो,
  मोन्टेनेग्रो/मोंटेनेग्रो, बुरुन्डी/बुरुण्डी, कालिन्चोक/कालिञ्चोक); no false merge.
- Golden sets on the released `world-np-lite.tgd` (keyed under the 1.0 fold), old engine and
  new: the world set 71 of 72 both (the same Varanasi case), the consumer's 83 of 83 both.
  `लन्डन`, `लण्डन` and `लंदन` each find London first with the new engine.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Fold न before any stop (Devanagari) | matches how the languages write the anusvara; 37 merges, all right | a fold change after 1.0 | **chosen**, with the 1.0 fold kept |
| Every class nasal before any stop | also folds म्ज | merges a real m with the anusvara | rejected |
| Alternates in the data only | no code change | needs every spelling of every name listed | rejected |
| Fold ड/द and ट/त | finds लंदन from लंडन | merges distinct native names | rejected |

## 5. What we build ourselves, and why

`teigeodata_names::fold` gains the rule; `fold_1_0` keeps the old one. `search_keys` adds the
1.0-fold exact and loose keys where they differ (a few keys per thousand names); the engine's
`key_hits` looks a query up under the 1.0 fold too, computed only when the query holds न and a
virama, so no other query allocates more. Tests: the fold's cases, the keys, and a pack keyed
each way searched by every spelling, each proven to fail with its part removed.

## 6. Impact on the plan

D-378. Phase 8: Q-037 done; the next Nepal data release carries the new keys.

## 7. Risks and open items

- The forge's matching (conflation, dedupe) uses the same fold, so the next build may join a
  few more same-name records; the build's gates measure it.
- A later fold change repeats this: keep the old fold's keys while packs built with it are in
  use.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Wikidata labels (ne, hi) | as pinned in `data/sources.lock` | 2026-11-22 | the forge's store |
| world-np-lite.tgd | data-2026.11.14.3 | 2026-11-22 | the docs data |

## 9. Sources

- Wikidata Nepali and Hindi place labels, CC0, via the pinned query results.
- [D-220](../../03-decisions/log.md): the 1.0 fold's nasal rule.
