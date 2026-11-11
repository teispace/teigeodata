---
title: "Nepali names for every place (TG-14)"
status: research
updated: 2026-11-12
owner: teigeodata maintainers
revisit: 2027-05-11
---

# Nepali names for every place (TG-14)

## 1. Question

Tithipatra's addendum ([consumer page](../../01-product/consumers/tithipatra.md), TG-14, D-213)
asks for a Nepali name in Devanagari and an English name for every place in the embedded layer:
Nepal's places (about 100k), the world's cities of 5,000 or more, and India's towns (about
8,000). Today 9.1% of Nepal's places (main) and 0.7% of the world part carry a Devanagari name
([sizes](../../05-data/sizes.md), "Names by script").

This note answers:

1. Which sources give Devanagari names, under which licence, and how many of our places they
   cover.
2. How to transliterate Latin to Devanagari for the places no source covers, which tools exist,
   and which output we may ship in the lite (CC BY 4.0, clean-room) build.
3. How to measure it, and what gate to set.
4. What to build in the forge, without code that names a country (P16).

It feeds TG-14 of the [embedded layer](embedded-layer.md) and extends
[names and search](names-and-search.md) and [canonical names](names-canonical.md), which cover
the opposite direction (Devanagari to Latin).

## 2. Summary and recommendation

- **A fallback chain, each name tagged with its origin:**
  1. official name (Nepal's local levels and districts: the CBS code list);
  2. Wikidata `ne` label;
  3. OSM `name:ne` (main build only);
  4. in India, the Wikidata `hi` label with a small Hindi-to-Nepali spelling pass;
  5. the GeoNames Survey Department romanisation (diacritic Latin), transliterated;
  6. the plain Latin canonical name, transliterated.

  Tiers 5 and 6 are machine output and carry `rule = transliteration`, so a consumer can hide
  them or show them differently.
- **Sources cover far less than the ask.** Wikidata has a `ne` label for 94% of its 4,780 Nepal
  settlements, but that is under 5% of Nepal's ~100k places. For world cities of 5,000 or more
  it has 1.4% `ne` and about 16% `hi` (§3.1). Transliteration is therefore the main path, not
  the edge case: about 77–80% of Nepal's clusters and about 84% of world cities (**estimate**,
  from cluster counts over all feature classes).
- **GeoNames `ne` is mostly not Nepali script.** Of 29,515 GeoNames alternate names tagged
  `ne`, only 1,059 are Devanagari. The rest are Survey Department style romanisations
  (Sumliṅṭār, Ḍā̃ḍāthok). Our read stage tags them `ne` today. They should be `ne-Latn` and must
  never count as a Nepali name.
- **Build our own transliterator; the tools tested are not usable.** On held-out pairs, ICU 79.1
  `Latin-Devanagari` and `indic-transliteration` (ITRANS) reach 0–3% exact. A prototype of the
  design below reaches 61% exact on the official local-level names and 37% on OSM pairs, with
  lite-clean data only (§3.3). The design: one generic engine, a per-language profile file of
  Latin-to-Devanagari options, a character language model trained on lite-clean Devanagari
  names, the country lexicon, and a word memory from paired names.
- **Hindi labels are good Nepali names inside India, poor ones outside it.** Wikidata `hi`
  matches `ne` on 78.5% of Indian cities (spelling-folded), against 27.3% elsewhere. Rules
  score only 17% on Indian towns. So use `hi` for India. Abroad, `hi` (28.7% exact after
  conversion) still beats rules on English spellings (7.1%). Use it there too, after the
  Nepali conversion, until a better source exists.
- **Gate:** measure exact match (strict and spelling-folded) and character error rate on a
  fixed held-out set of lite-clean pairs, with a floor that only rises. Report the noise
  ceiling beside it: two human sources (OSM and Wikidata) agree on only 46% of names (§3.4).
- **Learned models (IndicXlit) are an A/B oracle in the forge, not a dependency.** The code
  and weights are MIT. The Aksharantar training data is CC0 or CC BY. The model reports 52.7%
  top-1 on Nepali named entities. Its stack (fairseq, last PyPI release 2022-09-14) is heavy
  and old. Measuring it on our sets is **unverified**.
- **Country names** come from CLDR 49 (`ne.xml`, Unicode licence): exact and maintained.

## 3. Findings

### 3.1 Sources of Devanagari names and their coverage

All counts measured 2026-11-11 unless stated. Queries are in §5.4.

| Source | Licence | Builds | What it covers (measured) |
|---|---|---|---|
| CBS (now NSO) geographic code list, Open Data Nepal mirror | "Creative Commons Attribution" (portal record, metadata modified 2026-06-14) | lite, main | 753 local levels, 77 districts, 7 provinces, each with a Devanagari name and code. The province names are the old "प्रदेश १" form; take current province names from the constitution schedule or Wikidata |
| Wikidata labels `ne`, `hi` | CC0 | lite, main | Nepal settlements: 4,495 of 4,780 have `ne` (94.0%). Our pinned snapshot (`wikidata-np`): 5,410 of 9,586 items. World settlements with population ≥ 5,000: 1,150 of 71,715 `ne` (1.6%), 9,498 `hi` (13.2%) |
| GeoNames alternate names `ne`, `hi` | CC BY 4.0 | lite, main | `ne`: 29,515 rows, **1,059 Devanagari**. The rest are diacritic Latin romanisations. `hi`: 9,697 rows, 9,603 Devanagari |
| GeoNames Survey-style romanisations (tagged `ne`) | CC BY 4.0 | lite, main | 9,646 rows on Nepal's populated places. 26,791 of 163,363 conflated Nepal clusters carry one (all feature classes, mostly hydrographic and populated) |
| OSM `name:ne` | ODbL | main only | 27,124 objects world-wide (taginfo, data to 2026-11-11). In the Nepal extract, 7,546 `ne` names, 7,482 of them Devanagari. `name:hi` has 83,940 objects world-wide |
| MoFAGA local-level lists | none found | none | MoFAGA is the naming authority. We found PDFs only (notices) and no machine-readable list or licence ([Nepal sources](nepal-sources.md)) |
| Survey Department gazetteer | none found | none | No published Devanagari gazetteer. Ward and topographic data are sold, not published ([Nepal sources](nepal-sources.md)) |
| Census 2021 ward tables (NSO) | attribution (Q-011) | main | Ward names were not reachable from outside Nepal (Q-011) |
| India LGD (`villageNameLocal`) | **unverified** (data.gov.in pages did not state it in our search) | open | Names in each state's official language, not Hindi everywhere. Research with TG-15 |
| CLDR 49 `ne.xml` territory names | Unicode License v3 | lite, main | Every country: नेपाल, भारत, चीन, जापान, संयुक्त राज्य (alt short अमेरिका) |

**Coverage of the GeoNames `cities5000` list (70,026 places, downloaded 2026-11-11),** joining
GeoNames alternate names and Wikidata labels through Wikidata's GeoNames ID (P1566):

| Scope | Places | Wikidata `ne` | GeoNames `ne` (Devanagari) | Any `ne` | Any `hi` | `ne` or `hi` |
|---|--:|--:|--:|--:|--:|--:|
| World | 70,026 | 943 | 279 | 962 (1.4%) | 11,273 (16.1%) | 11,318 (16.2%) |
| Population ≥ 100,000 | 6,289 | 592 | – | 602 (9.6%) | 3,595 (57.2%) | 3,599 (57.2%) |
| India | 6,595 | 453 | – | 455 (6.9%) | 3,339 (50.6%) | 3,339 (50.6%) |
| Nepal | 54 | 48 | – | 50 | 34 | 50 |

Of the US places in `cities5000`, 1,867 carry a Devanagari alternate name in GeoNames. They are
Marathi (`mr`, 5,103 rows world-wide), not Nepali or Hindi, and are not used.

**Nepal, per conflated cluster** (the forge's `conflate` output, 163,363 clusters, every
feature class, so a different base from the pack's ~96.8k places):

| Tier | Clusters with a Devanagari or NSD name |
|---|--:|
| Wikidata `ne` | 5,406 |
| Wikidata or OSM (main) | 11,318 |
| + GeoNames NSD romanisation, lite (Wikidata + NSD) | 31,899 |
| + GeoNames NSD romanisation, main (Wikidata + OSM + NSD) | 37,635 |

The pack's own figure is 9.1% (main) Devanagari today ([sizes](../../05-data/sizes.md)).
Whatever the base, most of Nepal's places need tier 6.

### 3.2 Hindi labels as Nepali names

Wikidata items with both a `ne` and a `hi` label, among `cities5000`. "Folded" ignores
chandrabindu against anusvara, the nukta, ॉ against ो, and a nasal conjunct against anusvara.
CER is the Levenshtein distance over the folded reference length.

| Where | Pairs | Folded exact | CER |
|---|--:|--:|--:|
| India | 456 | 78.5% | 0.244 |
| Elsewhere | 450 | 27.3% | 0.664 |

The differences are spelling conventions, not errors, and they are regular:

- Hindi uses nukta letters (बग़दाद) and ॉ for English /ɒ/ (मेलबॉर्न); Nepali uses neither.
- Nepali writes English /v/ as भ (भिक्टोरिया against विक्टोरिया).
- Nepali writes velar nasals as conjuncts (किङ्स्टन against किंग्सटन).
- Hindi labels often carry qualifiers ("जिला", "शहर", ", अलाबामा"). Nepali ones carry
  "नगरपालिका", "उपमहानगरपालिका" for Nepal's municipalities.

A conversion pass (drop the nukta, ॉ → ो, ं before a velar → ङ्, strip qualifiers) changes the
folded score little (§3.3). The gain is in consistency with Nepali spelling, not in exact match
to Nepali Wikipedia.

### 3.3 Transliteration Latin → Devanagari: measured

**Test sets** (pairs of a Latin and a Devanagari name of the same place; test fold = BLAKE2 of
the ID mod 5 = 0, the rest trains):

| Set | Source | Licence | Pairs (all / test) |
|---|---|---|--:|
| `cbs` | CBS local levels (type word stripped) and districts | CC BY | 830 / 189 |
| `wd` | Wikidata Nepal, `en` and `ne` labels | CC0 | 4,688 / 953 |
| `osm` | OSM Nepal, `name`/`name:en` and `name:ne` (same query as [canonical names](names-canonical.md) §5.3) | ODbL, measure only | 6,737 / 1,377 |
| `nsd` | GeoNames NSD romanisation against OSM or Wikidata Devanagari, same cluster | CC BY / ODbL | 442 (all test) |
| `in` | `cities5000` India, GeoNames name against Wikidata `ne` | CC BY / CC0 | 448 |
| `world` | `cities5000` outside India and Nepal, same | CC BY / CC0 | 421 |

**Results** (Python prototype, 2026-11-11; "lite" means trained on the `cbs` and `wd` training
folds only, "main" adds the `osm` training fold):

| Set | Method | Exact | Exact, folded | CER |
|---|---|--:|--:|--:|
| cbs | ICU 79.1 `Latin-Devanagari` | 0.0% | 0.0% | 0.525 |
| cbs | `indic-transliteration` 2.3.82, ITRANS | 1.1% | 1.1% | 0.445 |
| cbs | profile + character LM (lite) | 60.8% | 62.4% | 0.174 |
| cbs | + lexicon + word memory (lite) | 61.9% | 64.0% | 0.172 |
| cbs | + word memory and LM with OSM (main) | 66.7% | 68.3% | 0.163 |
| wd | ICU 79.1 | 0.7% | 0.9% | 0.584 |
| wd | ITRANS | 2.2% | 2.6% | 0.510 |
| wd | profile + LM (lite) | 31.5% | 33.2% | 0.252 |
| wd | + lexicon + word memory (lite) | 34.8% | 36.2% | 0.231 |
| wd | main | 37.5% | 38.8% | 0.224 |
| osm | ICU 79.1 | 0.3% | 0.4% | 0.504 |
| osm | ITRANS | 2.8% | 3.2% | 0.412 |
| osm | profile + LM (lite) | 33.6% | 34.8% | 0.213 |
| osm | + lexicon (lite) | 36.3% | 37.3% | 0.199 |
| osm | + word memory (lite) | 37.2% | 38.5% | 0.199 |
| osm | main | 44.1% | 45.5% | 0.173 |
| nsd | plain profile on the ASCII-folded romanisation (lite) | 42.3% | 45.2% | 0.228 |
| nsd | NSD-aware profile (diacritics fix ā, ṭ, ḍ, ṇ, ṣ, ṅ, ñ, ँ) | 41.9% | 44.3% | 0.223 |
| in | profile + LM (lite) on the English name | 17.4% | 21.7% | 0.309 |
| in | Wikidata `hi` label as is | 76.8% | 81.9% | 0.078 |
| in | `hi` label, converted to Nepali spelling | 73.9% | 82.8% | 0.068 |
| world | profile + LM (lite) | 7.1% | 7.8% | 0.405 |
| world | `hi` label as is | 24.0% | 28.3% | 0.301 |
| world | `hi` label, converted | 26.6% | 28.7% | 0.293 |

What the numbers say:

- **ICU and ITRANS are not candidates.** They are reversible schemes for Sanskrit-style
  romanisation, not readers of informal spellings. They write a virama on every final
  consonant (ललित्पुर्) and never lengthen a vowel (पोखर for Pokhara).
- **The language model does most of the work.** It picks त or ट, द or ड, ि or ी, conjunct or
  inherent vowel, न् or ं. A 5-gram model on 5,827 lite Devanagari words already gives 61% on
  official names.
- **The lexicon helps where generic terms are frequent** (+2.7 points on OSM, which is rich in
  Chowk, Tole and Gaun). It is flat on the official names.
- **More training data helps:** adding the OSM fold lifts every set by 3–7 points. For lite,
  the same gain must come from CC0 and CC BY names: all Wikidata `ne` labels, the CBS list,
  and later the census ward names (Q-011).
- **NSD diacritics did not help on these references.** The romanisations and the OSM
  Devanagari disagree on the place itself (Lāṅjer against लङ्ग्जेर, Korālā against कोरला).
  The diacritics fix vowel length and retroflexion, but those were not the main errors. They
  stay useful as a source (26k clusters); their gain is **unverified** until a clean reference
  exists.
- **English spellings of foreign places do not transliterate** (7%: Wuhan, Tokyo, Boston follow
  English or pinyin orthography). For world cities, a Devanagari label from any Wikidata
  language in the script, converted, is the only reasonable non-manual source.

Errors that remain on the Nepal sets, by inspection of 100 misses:

| Kind | Example (input, reference, ours) |
|---|---|
| The reference is another name or a translation | Salghari → घर्तिगाउँ; Mohini waterfall → मोहिनी झरना |
| Vowel length | Piladi → पिलाडि, ours पिलाडी |
| Retroflex against dental | Dadhikot → दाधीकोत (reference) against दधिकोट |
| Conjunct against inherent vowel | Maranthana → मरन्ठाना, ours मारान्थान |
| ब against व | Bichalaa → विचला, ours बिचला |
| Velar nasal written three ways | Jubing → जुभिङ्ग, ours जुबिङ |

### 3.4 How noisy the references are

For the 968 Nepal clusters that carry both an OSM `name:ne` and a Wikidata `ne` label, the two
agree exactly on 46.0%, on 47.6% folded, and on 69.4% when one may be a prefix of the other
(most of the gap is "गाउँपालिका"-style suffixes). No method can score far above this on these
references. The official CBS list is the cleanest set we have, so the gate leads with it.

### 3.5 Linguistic pitfalls (Latin → Devanagari)

The [names and search](names-and-search.md) note §3.3 covers the reverse direction. In this
direction:

- **Schwa.** The Latin side has no letter for a deleted schwa. "Lalitpur" is ललितपुर, with an
  inherent vowel after त, not a conjunct (त्प). Each consonant pair is either a conjunct or
  two full letters, and only data decides. A written final "a" is usually ा (Pokhara, Khola),
  because a final inherent vowel is silent.
- **a / aa, i / ee, u / oo.** Writers use "a" for both अ and आ (Kathmandu is काठमाडौं), and "i"
  for both ि and ी.
- **Aspirates and retroflexes.** "th" is थ or ठ, "d" is द or ड, "dh" is ध or ढ. Writers never
  mark retroflexion.
- **w / v / b.** Nepali व is read [b] or [w], so "Biratnagar" is विराटनगर and "Bichalaa" is
  विचला. Foreign /v/ is often भ.
- **Nasals.** "n" before a consonant may be न्, ं, ँ or ङ्. A final "n" after a vowel may be ँ
  (Gaun → गाउँ). "ng" may be ङ, ङ्ग or ंग.
- **Sibilants.** "s" may be स, श or ष (Sisawa → शिशवा).
- **Compounds and spaces.** "Parajuligaun" against पराजुली गाउँ. Compare without spaces and
  hyphens.
- **Designators are translated, not transliterated:** "Rural Municipality" → गाउँपालिका,
  "River" → नदी, "Lake" → ताल.

### 3.6 Tools and their licences

| Tool | What it is | Licence | May its output ship in lite? |
|---|---|---|---|
| ICU4C `Latin-Devanagari` | Rule transliterator, ISO-like | Unicode-3.0 | Yes, but it is not usable (0–1% exact) |
| `indic-transliteration` (sanscript) | Scheme converter (ITRANS, HK, IAST) | MIT | Yes, but not usable (1–3%) |
| Aksharamukha | Script converter with "RomanColloquial" | AGPL-3.0 | Running it as a service is fine, but embedding it is not. Same class as ITRANS for this task ([names and search](names-and-search.md) §3.3) |
| AI4Bharat IndicXlit (`ai4bharat-transliteration` 1.1.3) | Transformer, about 11M parameters, 21 languages including Nepali | MIT (code and models) | Model output is not a copy of training data, and the weights are MIT, so yes. Its training set Aksharantar is CC0 (mined) and CC BY (manual). The reported Nepali top-1 is 80.3% on words and 52.7% on named entities (README) |
| Google Input Tools / Gboard | Learned input method | proprietary, online | No |
| Our profile + LM (prototype) | §5 | ours; the LM inherits its training data's licence class | Yes, when trained on lite-clean names only |

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Sources only (official, Wikidata, OSM, `hi`) | Human-written, exact | Under 5% of Nepal's places, 1.4% (`ne`) or 16% (`ne` or `hi`) of world cities | CC0 / CC BY / ODbL | – | **use first**, always |
| Deterministic rules (one output per letter) | Simple, no data | Cannot choose a/ā, t/ṭ, conjunct or schwa; ICU and ITRANS score 0–3% | – | ICU 79.1, sanscript 2.3.82 | no |
| Profile options + character LM + lexicon + word memory | 61% on official names, 37% on OSM (lite), deterministic, small (a profile and an n-gram table), runs in the forge, world-general by data | Ceiling set by noisy references; LM training data sets the licence | ours | prototype 2026-11-11 | **build** |
| IndicXlit (neural) | Strong on common words; Nepali covered | fairseq and PyTorch, last release 2022; 52.7% on named entities; not deterministic across library versions without pinning | MIT | 1.1.3 (2022-09-14) | A/B oracle in the forge only; adopt per language if it beats ours on the gate |
| `hi` label converted to Nepali spelling | 82.8% folded on Indian towns | Only where a `hi` label exists; Hindi conventions abroad | CC0 / CC BY | – | **use** in India; elsewhere above transliteration |
| Manual curation | Exact | Does not scale to 100k | ours | – | corrections layer, for the most important places |

## 5. What we build ourselves, and why

### 5.1 The fallback chain in the forge (`build`)

For each place and each target language in the profile (here `ne` in `Deva`):

1. **Official.** A name from an official list joined by code: `data/countries/<cc>/official-names`
   (for Nepal, the CBS list keyed by the CBS code already used as `ext.cbs`).
2. **Wikidata label** in the target language, joined by the Wikidata ID.
3. **OSM `name:<lang>`**, main build only.
4. **A related language's label**, from a per-target list of donor languages and the regions
   where each applies (for `ne`: `hi`, first in India, then elsewhere). It goes through the
   donor-to-target spelling pass (§5.2).
5. **A source romanisation with diacritics** (GeoNames NSD style), transliterated with the
   diacritic profile.
6. **The canonical Latin name**, transliterated.

Every chosen name records its tier in `rule` (schema: add `transliteration` and `donor`; the
names table in [schema](../../05-data/schema.md) lists the current values). Tiers 1–3 are
`official` or `local` names. Tiers 4–6 have type `alt` and must never become the canonical
name. A correction (forge §3.7) overrides any tier.

**Fix in `read` first:** tag GeoNames alternate names by script, not only by the source's
language code. Latin text tagged `ne` becomes `ne-Latn` (BCP 47). Today such names are counted
as Nepali in `lang` but not in the script-based size report.

### 5.2 Data files (P16: no language or country in code)

| File | Holds |
|---|---|
| `data/xlit/<lang>-Latn-<Script>.yaml` (first: `ne-Latn-Deva`) | Latin units → target options with costs (consonants, vowels as matra and independent forms, nasal signs, final-vowel and final-virama costs), designator translations, golden examples |
| `data/xlit/<lang>-Latn-<Script>.nsd.yaml` or a `variants:` block | The diacritic romanisation options (ā, ṭ, ḍ, ṇ, ṣ, ṅ, ñ, tilde → ँ) |
| `data/xlit/<donor>-<target>.yaml` (first: `hi-ne`) | Spelling pass: nukta drop, ॉ → ो, ं before a velar → ङ्, qualifiers to strip, regions where the donor applies |
| `data/countries/<cc>/lexicon.yaml` (exists) | Generic terms; the native forms are reused in this direction (Chowk → चोक) |
| Character LM and word memory | **Built** by the forge from the pinned names of the build's own licence class, never committed. Lite: Wikidata `ne`, CBS, GeoNames Devanagari. Main: plus OSM |

The engine (`teigeodata-names`, beside `romanise`) is one beam search over the profile's
lattice, scored by profile costs plus the LM. It names no language. A Hindi, Marathi or Bengali
target is a new profile file plus training names.

### 5.3 The gate (G-xlit)

- **Sets:** the CBS list (830 names) and the Wikidata Nepal pairs (4,688). The test fold is
  fixed by a hash of the ID, and the training fold never contains a test name. Main also
  reports the OSM pairs. India uses Wikidata `ne` against the converted `hi` labels.
- **Metrics:** exact (NFC, spaces and hyphens ignored), folded exact, and CER over the folded
  reference.
- **Floors,** set at the first Rust measurement and only rising:
  - v1 targets on `cbs`: folded exact ≥ 0.65, CER ≤ 0.15;
  - on `wd`: folded exact ≥ 0.40, CER ≤ 0.22.

  The prototype is at 0.640 / 0.172 and 0.362 / 0.231 (lite).
- **Noise ceiling** reported beside them: OSM against Wikidata agreement (46.0% exact,
  2026-11-11). A target above the ceiling would reward overfitting to one source.
- **Coverage report:** the share of places with a Devanagari name, by tier, per pack and per
  profile. In the embedded profile 100% must have one, and the transliterated share is
  printed so it can be driven down.
- **Round-trip check** (proposed, not measured): romanise each transliterated name with the
  existing `ne-Deva` profile. Flag it when the result does not fold to the input Latin name.
- **Property tests:** deterministic, never panics, output only in the target script's block
  plus space.
- **Prove the gate fails:** a profile with the LM switched off must fall below the floors (the
  prototype without the LM is at ICU's level).

### 5.4 Reproducing the measurements

Coverage of world settlements on QLever (`https://qlever.dev/api/wikidata`, 2026-11-11, 4.7 s):

```sparql
PREFIX wdt: <http://www.wikidata.org/prop/direct/>
PREFIX wd: <http://www.wikidata.org/entity/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT (COUNT(DISTINCT ?c) AS ?n) (COUNT(DISTINCT ?cne) AS ?ne) (COUNT(DISTINCT ?chi) AS ?hi) WHERE {
  ?c wdt:P31/wdt:P279* wd:Q486972 .
  ?c wdt:P1082 ?pop . FILTER(?pop >= 5000)
  ?c wdt:P625 ?coord .
  OPTIONAL { ?c rdfs:label ?l1 . FILTER(LANG(?l1) = "ne") BIND(?c AS ?cne) }
  OPTIONAL { ?c rdfs:label ?l2 . FILTER(LANG(?l2) = "hi") BIND(?c AS ?chi) }
}
```

Result: 71,715 / 1,150 / 9,498. Nepal: replace the population filter with
`?c wdt:P17 wd:Q837` → 4,780 / 4,495.

Labels by GeoNames ID, joined to `cities5000` in DuckDB:

```sparql
SELECT ?gn ?l WHERE { ?c wdt:P1566 ?gn . ?c rdfs:label ?l . FILTER(LANG(?l) = "ne") }
```

6,850 rows for `ne`, 28,223 for `hi`. GeoNames alternate names: the pinned
`geonames-alternate-names` file, rows whose language is `ne`, `hi`, `mr`, `new` or `mai`
(49,079 rows), with the script tested by the regex `[\x{0900}-\x{097F}]`.

### 5.5 Built: the engine, the profile and G-xlit (2026-11-12, D-222)

- **Engine:** `teigeodata_names::xlit` (`no_std` + `alloc`). A beam search (16 per Latin
  position) over the lattice of the profile's Latin units, scored by option costs plus
  `lm_weight` × the model's `-ln p` of every letter written and of the word's end; hypotheses
  with the same model context recombine. A consonant after a bare consonant either keeps the
  unwritten inherent vowel (`schwa`) or takes the virama (`conjunct`); a vowel is a sign after
  a consonant, an independent letter elsewhere; a written final vowel has its own `end` cost.
  Before the lattice: the profile's `words` (designators and pinned spellings, phrases
  first), the lexicon's terms, the word memory, then a generic term ending a word (Lahachowk:
  the stem transliterated, then चोक). Output is deterministic (two runs, byte-identical miss
  lists) and holds only the target block and single spaces. Per-call working memory
  (`Scratch`) is reusable.
- **Model:** a character 5-gram, interpolated Witten-Bell, stored in back-off form (sorted
  `u64` keys of packed symbols, `f32` costs; probabilities sum to 1, tested), plus a word
  memory from aligned pairs: a pair's words are aligned after the profile's phrases, a word
  enters when seen twice and the engine's own output for it is within a third of its length
  (so a misaligned pair or a typo label does not). Natural logs come from `libm`, so training
  gives the same model on every platform.
- **Profile:** [`data/xlit/ne-Latn-Deva.yaml`](../../../data/xlit/ne-Latn-Deva.yaml), checked
  by `cargo xtask check-data` (parse, name, compile with every lexicon).
- **Gate:** `forge xlit-eval ne-Latn-Deva` reads
  [`data/xlit/ne-Latn-Deva.gate.yaml`](../../../data/xlit/ne-Latn-Deva.gate.yaml): training
  sources, test sets, floors. Folds are BLAKE3 of the ID (not the prototype's BLAKE2) modulo
  5; fold 0 is test, `--dev` measures fold 1 and holds out both, for tuning without touching
  the test fold. Training never sees a held-out item nor any label holding a held-out
  reference as a run of whole words (stricter than the prototype, which only dropped the
  items). The examples are checked with the model trained on every fold, as a build would
  ship it. `--input <file>` writes a review list; `--misses <file>` the test misses;
  `--lm-weight 0` switches the model off.
- **Training data (lite):** `wikidata-labels-ne` (Wikidata `ne` labels of every item with a
  coordinate, CC0), the `ne` labels and aliases of `wikidata-np`, and the training folds of
  both sets. 11,646 labels used, 4,729 held out.
- **The CBS list is pinned:** `cbs-np-local-levels` (753 rows) and `cbs-np-districts` (77),
  Open Data Nepal, CC BY. The set strips the type word from both sides, the source's
  misspellings included ("Metropolitian"). The `wd` set cuts a comma's qualifier and a
  trailing designator from each side (`खलङ्गा, बैतडी` for Khalanga; `फुलेक गा.वि.स.`).

**Measured (lite, test fold, 2026-11-12, the shared 10-core build machine at load average 70–140):**

| Set | Pairs / test | Exact | Folded exact | CER | Floor (exact / folded / CER) |
|---|--:|--:|--:|--:|---|
| cbs | 830 / 159 | 44.0% | 44.7% | 0.123 | 0.440 / 0.446 / ≤ 0.124 |
| wd | 4,699 / 975 | 36.1% | 36.2% | 0.192 | 0.361 / 0.362 / ≤ 0.193 |
| cbs, model off (`--lm-weight 0`) | | 6.9% | 7.5% | 0.274 | fails |
| wd, model off | | 14.1% | 14.1% | 0.303 | fails |
| cbs, planted fault (`t` prefers ट) | | 40.3% | 40.9% | 0.149 | fails |
| wd, planted fault | | 33.1% | 33.2% | 0.206 | fails |

- **Model:** 81,543 n-gram and context entries and 17 memory words, 1.31 MB in memory
  (16 bytes an entry; not yet compressed or pruned). Training 0.2 s.
- **Speed:** 52–66 µs per name over the 1,134 test names at load average 70–140, and 145–300 µs
  at 500–700 (release build, one thread). The machine was shared and loaded throughout, so
  these are upper bounds; a quiet-machine bench is open.
- **Against the prototype** (§3.3: cbs 64.0% folded, wd 36.2%): `wd` is level; `cbs` is 20
  points lower. The stricter hold-out is the likely cause: CBS names recur in Wikidata labels
  ("फक्ताङ्लुङ्ग गाउँपालिका"), which the prototype's LM saw. The tuning on the dev fold
  (lm weight 0.6 against 1.0: +2 to +3 points; conjunct cost 0 against 0.3: +0.5; order 4 or
  6, unit cost changes: within ±1) did not close it. Adding Wikidata `hi` labels to the model
  lowered `wd` by 5.6 points, so Hindi stays out of the Nepali model. The v1 targets of §5.3
  (cbs 0.65, wd 0.40 folded) are not met; more lite-clean Nepali text (census ward names,
  Q-011; GeoNames Devanagari `ne`) and the word memory from more pairs are the next levers.
- **Pinned spellings:** the model writes लाहा for Laha and बिराटनगर for Biratnagar. The CBS
  list itself writes बिराटनगर, and Wikidata is split; the maintainer's examples (लहाचोक,
  विराटनगर) and Kathmandu's irregular काठमाडौं are pinned in the profile's `words`. Biratnagar
  falls in the `wd` test fold, so the pin adds one exact match there (0.1 point).

### 5.6 Proposal: the name-origin marker (not built)

NAMES already has an optional per-record column `RULE` (id 9, a `u32`, 0 = none), which no
build writes yet, and the schema lists its intended values in words only. Proposal:

- Give `RULE` a registry of codes in `teigeodata-format` (`NameRule`), stable once released:
  1 `official`, 2 `signage`, 3 `lexicon`, 4 `osm_majority`, 5 `geonames`, 6 `wikidata`,
  7 `romanisation`, 8 `transliteration` (§5.1 tiers 5 and 6), 9 `donor` (tier 4, a related
  language's label after the spelling pass). Unknown codes read as "other", never an error,
  so a later code does not break an older reader.
- Widen its meaning from "which step chose the canonical name" to "how this record was
  made", for every record. Codes 7 to 9 are machine output.
- The engine answers `Name::is_machine()`; search and display may rank machine names below
  human ones; an embed profile (`data/embed/<name>.yaml`) gains `machine_names: keep | mark |
  drop`, so Tithipatra can hide them or show them differently.
- `forge` writes the code where it adds the name; `check-data`'s golden gate refuses a
  machine name that became canonical (D-215).

## 6. Impact on the plan

- **TG-14:** the chain of §5.1 and the gate of §5.3. Order:
  1. the `read` script-tagging fix;
  2. tiers 1–2 (CBS list, Wikidata);
  3. the transliteration engine with the `ne-Latn-Deva` profile;
  4. the `hi-ne` donor pass when India lands (TG-15).
- **Sources to pin** in `data/sources.lock`:
  - the CBS code list (Open Data Nepal, CC BY): pinned 2026-11-12 (§5.5);
  - Wikidata `ne`/`hi` labels for the world part (QLever snapshot, CC0);
  - CLDR 49 `ne.xml`;
  - GeoNames `cities5000` (TG-13).
- **Schema:** `rule` gains `transliteration` and `donor`; GeoNames romanisations become
  `ne-Latn`.
- **Decision to record:** the chain, the rule that machine names are never canonical, and the
  lite LM's training classes.
- **Sizes:** a Devanagari name for every place adds about 10–20 bytes per place before
  compression (**estimate**). Measure it with `pack-sizes --langs ne,en` after the build.

## 7. Risks and open items

- **Users will see machine spellings.** At 35–65% exact, a third or more of the transliterated
  names differ from what a Nepali writer would choose. Tier tagging lets Tithipatra mark them.
  Corrections fix the most-viewed places first.
- **Reference quality bounds the gate.** A clean labelled sample (Q-017) of a few hundred
  places spelled by a native speaker would give a true accuracy figure.
- **Licence of a trained LM.** We treat an LM and word memory trained on OSM as ODbL-derived,
  so main only. Lite uses only CC0 and CC BY names (CLEAN_ROOM rule 2). This is conservative
  and not a legal opinion.
- **CBS list currency.** It has the old province names and may predate later renamings of
  local levels. Cross-check it against Wikidata and MoFAGA notices, and record mismatches as
  corrections.
- **MoFAGA and Survey Department** publish no machine-readable Devanagari list with a licence
  (searched 2026-11-11). Ask MoFAGA (maintainer).
- **India:** LGD local-language names (`villageNameLocal`) may be the official tier for India.
  Their licence and language per state are **unverified** (TG-15).
- **NSD romanisations' gain is unverified** on a clean reference (§3.3).
- **Newar, Maithili and Tharu names** written in Latin follow other phonology. The Nepali
  profile will misspell some. A per-place correction or a language-specific profile fixes
  them.
- **IndicXlit was not run** here (a heavy install). An A/B in the forge on the §5.3 sets is the
  test before any claim.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| ICU4C (`uconv`) | 79.1 (released 2026-10-22) | 2026-11-11 | local Homebrew `icu4c@79`; GitHub releases API |
| indic-transliteration | 2.3.82 | 2026-11-11 | PyPI |
| ai4bharat-transliteration (IndicXlit) | 1.1.3 (uploaded 2022-09-14), MIT | 2026-11-11 | PyPI JSON API; github.com/AI4Bharat/IndicXlit |
| Aksharantar | Hugging Face card last modified 2023-08-31; CC0 (mined) and CC BY (manual) | 2026-11-11 | huggingface.co/datasets/ai4bharat/Aksharantar |
| Aksharamukha | 2.3, AGPL-3.0 | 2026-11-11 | PyPI |
| CLDR | release-49 (2026-10-21) | 2026-11-11 | GitHub releases API; `common/main/ne.xml` |
| GeoNames `cities5000.zip` | 70,026 rows; SHA-256 prefix `1937668153a6b5c8` | 2026-11-11 | download.geonames.org |
| GeoNames `alternateNamesV2.zip` | pinned, file dated 2026-11-06 | 2026-11-11 | `data/sources.lock` |
| CBS code list (Open Data Nepal) | metadata modified 2026-06-14; local-level CSV SHA-256 prefix `6d580ab7e80cc7a6` | 2026-11-11 | api.opendatanepal.com CKAN |
| Wikidata (QLever) | live snapshot | 2026-11-11 | qlever.dev/api/wikidata |
| OSM taginfo | data until 2026-11-11 | 2026-11-11 | taginfo.openstreetmap.org API |
| DuckDB CLI | 2.0.0 | 2026-11-11 | local Homebrew |
| Python (prototype) | 3.15.2 | 2026-11-11 | local |

## 9. Sources

- [Names, romanisation and search](names-and-search.md) and [canonical names](names-canonical.md),
  this repository: the reverse direction, its linguistic sources and the `ne-Deva` profile.
- [Nepal sources](nepal-sources.md), this repository: CBS codes, MoFAGA, Survey Department,
  census.
- [Geographical Codes for States, Districts and Local Bodies](https://opendatanepal.com/dataset/geographical-codes-for-states-districts-and-local-bodies),
  Open Data Nepal (harvested from CBS), read via the CKAN API 2026-11-11.
- [GeoNames data dumps](https://download.geonames.org/export/dump/), GeoNames, CC BY 4.0,
  accessed 2026-11-11.
- [QLever Wikidata endpoint](https://qlever.dev/wikidata), University of Freiburg, accessed
  2026-11-11. Wikidata data is CC0.
- [taginfo: name:ne](https://taginfo.openstreetmap.org/keys/name:ne) and
  [name:hi](https://taginfo.openstreetmap.org/keys/name:hi), OpenStreetMap, accessed
  2026-11-11.
- [IndicXlit](https://github.com/AI4Bharat/IndicXlit), AI4Bharat, README (MIT; Nepali top-1
  80.25 words, 52.67 named entities), accessed 2026-11-11. Madhani et al., "Aksharantar:
  Open Indic-language Transliteration Datasets and Models for the Next Billion Users", 2022.
- [Aksharantar dataset card](https://huggingface.co/datasets/ai4bharat/Aksharantar),
  AI4Bharat, accessed 2026-11-11.
- [ai4bharat-transliteration on PyPI](https://pypi.org/project/ai4bharat-transliteration/),
  accessed 2026-11-11.
- [indic-transliteration on PyPI](https://pypi.org/project/indic-transliteration/), accessed
  2026-11-11.
- [Aksharamukha on PyPI](https://pypi.org/project/aksharamukha/), accessed 2026-11-11.
- [ICU transforms](https://unicode-org.github.io/icu/userguide/transforms/general/), Unicode
  Consortium, accessed 2026-11-11.
- [CLDR release 49, `ne.xml`](https://github.com/unicode-org/cldr/blob/release-49/common/main/ne.xml),
  Unicode Consortium, accessed 2026-11-11.
- [nepal-locallevel-map](https://github.com/younginnovations/nepal-locallevel-map), Young
  Innovations: Nepali local-level names compiled from MoFALD and OKNP pages; MIT on the
  repository, no data licence. Not used. Accessed 2026-11-11.
- [Local Government Directory: villages](https://www.data.gov.in/resource/local-government-directory-lgd-villages),
  data.gov.in: `villageNameLocal` field; licence not confirmed. Accessed 2026-11-11.
- [Transliteration system for Nepali language](https://nepjol.info/index.php/NJG/article/view/23114),
  S. M. Shrestha, Nepalese Journal on Geoinformatics: the Survey Department (NSD)
  romanisation behind GeoNames' diacritic forms.
- [Romanization of Nepali](https://geonames.nga.mil/geonames/GNSSearch/GNSDocs/romanization/ROMANIZATION_OF_NEPALI.pdf),
  BGN/PCGN 2011 (adopts the NSD system), accessed 2026-11-11.
