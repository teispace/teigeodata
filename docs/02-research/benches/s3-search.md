---
title: "Spike S3: name search on the Nepal golden set"
status: research
updated: 2026-11-06
---

# Spike S3: name search on the Nepal golden set

**Question:** can FST indexes plus the phonetic key, a fuzzy automaton and prefix search meet
the search gates of [ADR-0032](../../03-decisions/adr/ADR-0032-text-search.md) offline, in a
few MB, with every country rule read from data ([principle P16](../../00-vision/principles.md))?

**Input:**
- GeoNames `NP.txt` (CC BY 4.0) and OCHA COD-AB Nepal v02 (`npl_admin_boundaries.xlsx`, SHA-256
  `b9798479…1240be`). Together that is 130,574 records in 130,054 entity groups.
- The country folder `data/countries/np/` (profile, lexicon, curated queries).
- The golden set `tests/golden/np.yaml`: 512 queries (500 generated with a fixed seed plus 12
  curated), SHA-256 `c5d74101…c38c150`.

**Code:** `spikes/s3-search`. The run is
`cargo run --release -p spike-s3-search --bin s3-eval -- np <NP.txt> <COD-AB xlsx>`.
`S3_MISSES=1` lists every miss. The A/B switches are documented in the binary.
`S3_ALIASES=<tsv>` adds the forge `aliases` output to the index. With it, recall@1 is 0.959 and
recall@5 0.994 ([S5](s5-nepal.md#aliases-into-search-s55-third-part), D-060).

**Machine:** Apple M2 Pro, shared (load 3–10 during the runs), single thread. A query's
latency is the median of 21 runs; p50 and p99 are taken over the queries.

**Latency varies about 2× between runs** of the same binary, with machine load and with which
cores the scheduler picks. Overall p50/p99 for the chosen design:

| Run | p50 µs | p99 µs |
|---|---:|---:|
| quietest run (the table below) | 26 | 78 |
| later runs | 46 | 116 |
| forced onto efficiency cores (`taskpolicy -b`) | 75 | — |

Recall numbers do not vary. This is why [the benchmark method](../../06-engineering/benchmarks.md)
gates on instruction counts; Phase 2 re-measures search (benchmark Q6) that way.

## Design measured

1. **Normalise:** the per-script search fold.
2. **Context:** "Name, Parent" splits on the comma. A parent match outranks everything else.
3. **Abbreviation:** a trailing unit-type abbreviation from the profile ("RM", "SMC") is
   stripped and adds a unit-type bonus.
4. **Mixed script:** a native-script generic at the end of the query is replaced by each of its
   Latin spellings from the lexicon ("Anantaपुर" → "Anantapur"). The phonetic key also classifies
   each script run within a word on its own.
5. **Candidates.** Three `fst` maps (key → postings of record indexes):

   | Map | Keys |
   |---|---|
   | exact | folded names and alternates |
   | loose | spaces removed, and the name without a droppable suffix from the profile |
   | phonetic | the `indic-v0` key |

   - **Fuzzy:** our own Damerau (OSA) automaton walks the exact FST, allowing one edit. The
     state is in fixed arrays, so there is no allocation per transition.
   - **Anchoring:** the first letter must match. When nothing exact, loose or within one edit
     is found, the automaton runs again unanchored.
   - **Prefix:** the first 256 keys under the query's prefix.
6. **Score:** match bonus plus importance (0–130, from feature class and log population). The
   bonuses, one set for every country:

   | Match | Bonus |
   |---|---:|
   | exact | 60 |
   | loose | 55 |
   | one edit | 25 |
   | prefix | 30 |
   | unit-type match | +20 |
   | phonetic | 5 |

   Results are entity groups (one entity across sources), top 10.

## Results

**Index:** 3,633,491 bytes for 130,574 records, about 28 bytes per record. The postings are
uncompressed `u32`.

| Map | Bytes |
|---|---:|
| exact | 1,368,383 |
| loose | 1,351,474 |
| phonetic | 913,634 |

Build takes 0.37 s; loading the sources takes 0.1 s.

| Class | n | recall@1 | recall@5 | pass@max_rank | MRR@10 | nDCG@10 | p50 µs | p99 µs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| abbreviation | 41 | 0.976 | 1.000 | 1.000 | 0.984 | 0.988 | 27.0 | 78.1 |
| ambiguous | 41 | 0.976 | 1.000 | 0.976 | 0.988 | 0.967 | 27.8 | 74.8 |
| colloquial | 14 | 0.714 | 0.857 | 0.786 | 0.774 | 0.795 | 21.5 | 45.7 |
| exact_latin | 70 | 0.986 | 1.000 | 0.986 | 0.993 | 0.995 | 26.9 | 41.6 |
| exact_native | 42 | 0.952 | 0.952 | 0.952 | 0.952 | 0.952 | 12.6 | 25.6 |
| historical | 44 | 0.932 | 0.955 | 0.955 | 0.943 | 0.946 | 33.3 | 97.1 |
| mixed_script | 35 | 0.914 | 1.000 | 1.000 | 0.957 | 0.968 | 36.4 | 67.8 |
| prefix | 40 | 0.900 | 1.000 | 1.000 | 0.950 | 0.963 | 24.8 | 84.1 |
| typo | 80 | 0.963 | 1.000 | 1.000 | 0.981 | 0.986 | 20.5 | 42.8 |
| variant | 45 | 0.956 | 0.978 | 0.978 | 0.963 | 0.967 | 26.4 | 108.9 |
| with_context | 60 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 34.2 | 60.8 |
| **all** | 512 | **0.953** | **0.986** | 0.980 | **0.969** | 0.971 | **26.4** | **78.1** |

**Against the proposed gates:**

| Gate | Target | Measured | |
|---|---|---|---|
| Exact classes, recall@1 | ≥ 0.95 | exact_latin 0.986, exact_native 0.952 | met |
| variant, typo, mixed_script, recall@5 | ≥ 0.90 | 0.978, 1.000, 1.000 | met |
| MRR@10 | ≥ 0.85 | 0.969 | met |
| p99 | < 100 µs | 78–116 µs overall (load-dependent) | not reliably met; variant 109 µs in the quiet run |

`colloquial` and `historical` are below 0.95 because of data, not the engine (see the misses
below).

**Probe outside the golden set:** a first-letter keyboard typo on each exact_latin query. The
generated `typo` class never edits the first letter, so this probe is what measures the cost
of anchoring. Result: recall@5 0.900 (63/70), against 1.000 unanchored.

**Reproducible:** two runs give the same metrics, and the golden file regenerates
byte-identical.

## A/B: what each part buys

| Variant | recall@1 | recall@5 | MRR@10 | p50 µs | p99 µs | first-letter probe |
|---|---:|---:|---:|---:|---:|---:|
| **chosen** (above) | 0.953 | 0.986 | 0.969 | 26 | 78 | 0.900 |
| exact and loose only (no fuzzy, no prefix) | 0.725 | 0.783 | 0.751 | 2 | 12 | — |
| no fuzzy | 0.814 | 0.861 | 0.835 | 4 | 27 | 0.143 |
| no prefix | 0.887 | 0.910 | 0.898 | 43\* | 118\* | 0.900 |
| no phonetic | 0.949 | 0.980 | 0.964 | 44\* | 106\* | 0.857 |
| phonetic bonus 35 (key ranks above fuzzy) | 0.908 | 0.986 | 0.943 | 44\* | 102\* | 0.800 |
| unanchored fuzzy | 0.953 | 0.986 | 0.969 | 50 | 105 | 1.000 |
| plus a two-edit fallback from 8 bytes | 0.953 | 0.986 | 0.969 | 45\* | 138\* | 0.900 |
| `fst` crate `Levenshtein` (DFA), 1–2 edits | 0.895 | 0.986 | 0.935 | 293 | 707 | — |

\* Measured while the machine was more heavily loaded. Compare the recall columns across rows,
and the latency only against runs made at the same time.

**Findings:**

- **Fuzzy matching carries the most weight.** Without it recall@5 falls to 0.861.
- **The `fst` crate's Levenshtein DFA costs about 10× our automaton.** Building the DFA per
  query dominates. Our Damerau automaton also counts transpositions, which 4 of the 6 earlier
  typo misses needed ("Ksipang" for Kispang).
- **Two edits buy nothing on this set and cost p99.** They stay off, as an option behind a
  switch.
- **The phonetic key must rank below fuzzy and prefix.** At +35 it caused false merges that
  pushed true matches to rank 2 (recall@1 0.908). At +5 it is a safety net: it gains 0.006
  recall@5 and recovers first-letter typos that fuzzy anchoring misses (Cathmandu → K).
- **Anchoring halves the latency** but loses 10% of first-letter typos. Production should get
  both with a faster automaton (bit-parallel, Myers/Hyyrö) or per-node pruning, measured in
  Phase 2 (benchmark Q6). Until then anchored plus fallback is the default.
- **Prefix needs a precomputed top-k.** Reading 256 keys under the prefix and ranking by
  importance works on Nepal. At world scale, prefix needs ADR-0032's top-k packed in the FST
  value.

## Phonetic key `indic-v0` pair sets

Positive pairs are every pair of spellings of one lexicon term, Latin and native: 303 pairs.
Negative pairs are distinct folded names of different entities under the same parent: 34.5
million pairs.

| Key | Pair recall | False-merge rate | Bucket p50 | Bucket p99 | Max | Golden recall@1 |
|---|---:|---:|---:|---:|---:|---:|
| **indic-v0** (with the ङ fix) | 0.789 | 0.00010 | 1 | 33 | 672 | 0.953 |
| indic-v1 (drop word-final I/U) | 0.855 | 0.00016 | 1 | 39 | 698 | 0.949 |

v1 raises pair recall, but it also raises false merges by 60% and bucket p99 by 18%, with no
golden gain. Under the rule in names-and-search §5.3 it is rejected for now. It is re-tested on
the OSM `name:ne`/`name:en` pairs in S5, which test native names far better than 303 lexicon
pairs.

**Rules this spike fixed in the key (names-and-search §5.3 updated):**
1. Nasals are resolved before the non-initial A is dropped.
2. Devanagari word-final schwa is deleted, as Nepali and Hindi speak it.
3. A written velar nasal (ङ, final "ng") is kept unless a consonant follows (ङ्ग is
   homorganic).
4. Each script run in a mixed word is classified on its own.

**Remaining positive misses** are mostly not phonetic:
- translations: Ward/Wada, Than/Sthan/Asthan;
- the old British "u" for schwa: Gunj/Ganj;
- missing letters: Bhanjyan.

Production runs lexicon canonicalisation *before* the key (as §5.3 specifies), which removes the
lexicon-pair misses by construction. The spike measured the raw key on purpose.

## Misses and what fixes them

| Query | Want | Cause | Fix |
|---|---|---|---|
| Khwopa, Yen, Yambu, Kasthamandap | Bhaktapur, Kathmandu | aliases absent from GeoNames | S5: Wikidata and OSM `alt_name`/`old_name`; curated aliases |
| 𑐏𑑂𑐰𑐥𑑅, 𑐫𑐾𑑃 (Newa script) | Bhaktapur, Kathmandu | no Newa names in GeoNames | S5: OSM `name:new`, Wikidata |
| Patan | Pātan (1282931) | Patan and Lalitpur Metropolitan City are two groups | conflation (forge): city and seat |
| Sindhupalchok | the COD-AB unit | GeoNames "Sindhupalchowk" was not joined | conflation through the lexicon (chok = chowk) |
| Kuse | Kuse [Morang] | Koshi Province ranks first | ambiguity; importance from pageviews (ADR-0032) |
| Milanchouk | Milanchowk [Syangja] | three places share the key; the most important wins | ambiguity; inherent |

**Data fix found by this spike:** the COD-AB join compared folded names with spaces. "Necha
Salyan" (GeoNames) and "Nechasalyan" (COD-AB) stayed apart, so the same municipality appeared
twice. The join key is now the fold without spaces, the golden set was regenerated, and 23
records joined their groups (130,077 → 130,054 groups).

## Conclusions

1. **The design meets every recall gate on Nepal** in 3.6 MB: FST maps, anchored Damerau fuzzy,
   prefix, a low-weight phonetic key, and one scoring formula for every country. Latency is
   26–46 µs p50 and 78–116 µs p99 on a shared laptop, so the 100 µs p99 gate is met only on a
   quiet machine. The fuzzy automaton is most of the cost and the first thing Phase 2
   optimises.
2. **The search engine is not the limit; the data is.** 7 of the 12 curated queries fail on
   missing aliases or unmerged entities. S5 (aliases) and the forge's conflation are where the
   remaining recall is.
3. **Before Phase 2:**
   - a faster unanchored automaton;
   - top-k prefix in the FST value;
   - compressed postings;
   - tiers searched per token as well as for the whole name;
   - the bonuses re-fitted on a held-out set (India, and S5's OSM pairs), because these were
     tuned on this golden set. The tuning was light (one sweep of the phonetic bonus), and
     results were checked against the probe, which is not part of the golden set.
