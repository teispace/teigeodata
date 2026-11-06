---
title: "Names, romanisation and search"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-05-06
---

# Names, romanisation and search

## 1. Question

teigeodata stores, for every place in Nepal and India, an English/Latin name, the official local-script name (Nepali in Devanagari; for India, Hindi plus the state's official language), and sourced local endonyms. Search has to find the place whatever the user types: काठमाडौं, Kathmandu, Kathamandu, Kantipur, `ktm`, typos, mixed scripts. It must run offline, in a few MB, with queries under 100 µs.

This note answers five questions. Each one feeds a decision.

1. **Romanisation.** Which romanisation system, if any, should the canonical Latin name follow? Which one matches signage and common use?
2. **Canonical spelling.** When sources disagree (Lahachowk vs Lahachok), how do we pick the display spelling? What does the contributor-editable generic-terms lexicon look like?
3. **Transliteration.** How do we convert Devanagari to Latin and back for indexing? Which existing libraries help?
4. **Phonetic key.** What phonetic key works from either script and collapses the common variants?
5. **Search and evaluation.** What search architecture, Unicode handling and evaluation plan meet the size and latency budget?

It feeds the ADRs on name model, canonical spelling, search index and text normalisation (section 6). It also feeds the golden-test gate for the first Nepal release.

## 2. Summary and recommendation

- **No published romanisation standard matches signage.** For Nepal, the national system is the Nepal Survey Department (NSD) system. BGN/PCGN adopted it in 2011. It is a diacritic transliteration (`kāṭhmāḍauṁ`-style), not the spelling on signs. For India, the official system is Hunterian, via the Survey of India: Devanagari form first, then Hunterian. Its map form uses macrons. The UN-approved systems for Nepali and Hindi (1972/1977) are not used in either country. BGN/PCGN has no Hindi system at all. ISO 15919 and IAST are scholarly and reversible, and useless for display. So **the canonical display spelling is a curated convention, not a transliteration.** It is in effect Hunterian or NSD with the diacritics stripped, plus established spellings of generic terms (Chowk, Bazar, Gaun, Tole, Bhanjyang) and per-place official overrides (Birgunj, Nepalgunj, Bengaluru).
- **Canonical spelling precedence.** Per place, use this order:
  1. An official spelling recorded on that entity (a gazette or government register: MoFAGA local-level names for Nepal, the Local Government Directory or Survey of India for India).
  2. Documented signage.
  3. The generic-terms lexicon applied to the source name. This step is what makes "Lahachok" render as "Lahachowk".
  4. OSM majority in the area.
  5. GeoNames, then Wikidata/Wikipedia English labels.
  6. Deterministic romanisation from the Devanagari name.

  Every losing spelling is kept as an `alt` name, so search still finds it.
- **The lexicon is a YAML file keyed by country.** Canonical spellings differ by country: Nepal uses *Gaun* (OSM Nepal: 377 name occurrences vs 14 for *gaon*), India uses *Gaon*; Nepal uses *Gadhi*, India *Garhi*. Section 5.2 gives a starter list of 60 terms, and section 5.5 the schema.
- **Transliteration: write our own small rule tables.** We need a "readable" Devanagari→Latin profile with Nepali/Hindi schwa deletion and NSD/Hunterian consonant choices. We do not want ISO 15919. ICU's `Devanagari-Latin` gives `lahācōka` and `kāṭhamāḍauṁ`. Sanscript gives `lahācoka`. Aksharamukha's "RomanColloquial" gives `lahachoka` and `kathamadaum`. All keep the schwa and none matches signage (measured, section 3.3). The rules should be in a data format that ICU4X's experimental transliterator could also run (**unverified** that it can load our rules unmodified).
- **Phonetic key: our own ordered folding.** The key is in the spirit of IndicSOUNDEX (DiPersio 2021). It differs in two ways: it drops every non-initial *a*-class vowel, which makes schwa deletion irrelevant, and it merges o/u and e/i. It runs from Latin or Devanagari to the same alphabet. The key generates candidates and is never the final ranker. Section 5.3 has the draft.
- **Search architecture.**
  - One `fst::Map` per normalised form: Latin-folded, Devanagari-folded, phonetic key and alias.
  - Fuzzy matching with Levenshtein automata, with the edit budget set by query length.
  - Top-k prefix completion using scores packed into the FST value, plus a precomputed top-k table for prefixes of 1–2 characters.
  - Ranking: match quality, then importance (admin level, log population, Wikipedia pageviews, OSM tag richness), then proximity if a focus point is given, then name type.
  - SymSpell and BM25 are not the primary path (section 4).
- **Unicode.**
  - Store NFC.
  - Search-fold with NFKC + case fold for Latin.
  - For Devanagari, fold nukta, chandrabindu→anusvara and ZWJ/ZWNJ for search only. Do prefix matching on code points, not grapheme clusters.
  - Apply UTS #39 mixed-script checks to contributor names at build time.
- **Golden set.** At least 500 graded queries per country, in 11 query classes. Report recall@1, recall@5 and MRR@10 per class, plus latency p50/p99 and index size. Use it as a CI gate.

**Unverified items are marked in place.** The main ones: the OSM Nepal tagging spreadsheet (not read), whether the Survey of India still uses Hunterian after 2006, the details of Google and Apple behaviour, and the ICU4X rule loading.

## 3. Findings

### 3.1 Romanisation systems

**Nepal Survey Department (NSD) system / BGN/PCGN 2011.** The BGN/PCGN agreement says: "The BGN and the PCGN have adopted the Nepal Survey Department (NSD) system for the romanization of Nepali names". It applies "to Nepali names for which Roman-script spellings in materials produced by the government of Nepal are not available". So government Roman spellings take precedence even for BGN/PCGN, which supports our entity-override rule. The key rules in the table:

- च = ch, छ = chh, श = sh, ष = ṣ, क्ष = kṣ, ज्ञ = jñ.
- व = "v/w". The note says v is primary and w "is believed to be dependent on dialect, pronunciation, or stress".
- ऋ = ṛi.
- Chandrabindu is "~ over the vowel". Anusvara is assimilated: ṅ/ñ/ṇ/n before the matching stop class, ṁ before y, r, l, v, sh, s, h.
- Visarga = h. ड/ढ are optionally ṛ/ṛh where they stand for a flap, as in पहाड → pahāṛ.
- The halanta is "not normally written in Nepali ... Knowledge of the pronunciation of Nepali names is necessary for the proper treatment of the inherent short [a] vowel". Schwa deletion is left to the human.

The PDF (checked for validity October 2017) has an obvious error: it prints ◌ॊ (short o) for ओ's medial form. It has no generic-term list.

The UNGEGN WGRS status report (Nepali, v4.0, February 2013) confirms three things. The UN system (1972, res. II/11, based on D. N. Sharma) has never been implemented in Nepal. NSD's system is used on national mapping and in the National Geographic Information Infrastructure Programme (NGIIP). The old BGN/PCGN 1964/1988 system was retired in 2009. The report's simplified form drops diacritics: ṅ→ng, ṣ→sh, kṣ→ksh or x, jñ→gy.

**Practice in Nepal is inconsistent even within the Survey Department.** S. M. Shrestha, in *Nepalese Journal on Geoinformatics* no. 11 (BS 2069 ≈ 2012), shows Kathmandu as "KĀṬHMĀṆḌAU" on the 1:25,000 topographic sheets and "KĀṬHMĀṆḌU" on the 1:1,000,000 map. The Nepali-language maps carry multiple Devanagari spellings. He calls for a national Geographical Names Authority. I found no evidence that one was created (**unverified**). The note does not cover generic terms.

**Hunterian / Survey of India.** W. W. Hunter devised the system in the 1860s and published his *Guide to the Orthography of Indian Proper Names* in 1871. The Government of India adopted it with modifications in 1872. The UNGEGN WGRS India page describes the procedure:

1. State authorities collect names in the original script.
2. The Survey of India transcribes them into a standard Devanagari form, "basically Hindi".
3. The Survey of India romanises that form with Hunterian, labelled "English".
4. The Survey of India is the final authority. A shrinking number of names stay anglicised.

The page also records that "in May 2006, the Survey of India confirmed to the United Kingdom" its continued use. I found no later confirmation (**unverified** after 2006).

Hunterian uses ch/chh and maps all consonants to plain letters. Long vowels take a macron, but not word-finally. It is not reversible. The Survey of India guidelines, as summarised in a community gist, add three map rules. A final a/i/u carries no accent. Village names ending in *-pur* carry no accent on the u. Descriptive feature words are written apart from the specific (a hill, "Chenni Malai") but joined for villages ("Chennimalai"). That last rule is directly relevant to Lahachowk vs "Laha Chowk" (secondary source, **unverified** against the Survey of India original).

**UN system for Hindi.** Approved in 1972 (II/11) and amended in 1977 (III/12). WGRS Hindi v4.0 (March 2016) finds no evidence of use in India: Hunterian remains in use for large-scale mapping. The UN system romanises the inherent a except at the end of a name. It points to ISO 15919 for other Indic scripts.

**BGN/PCGN Hindi.** There is none. The UK government's list of romanisation systems (June 2023) has Nepali (Agreement, 2011, Nepal Survey Department) and Urdu, but no Hindi or any other Indian-script system.

**ISO 15919 (2001) and IAST.** Both are reversible scholarly transliterations with diacritics. ISO 15919 distinguishes ē/e and ō/o for Dravidian scripts, so it writes Devanagari ए as ē. Neither deletes schwa. ISO's catalogue page refused automated access, so the current review status is **unverified**. Their value to us is as an internal pivot (IndicSOUNDEX uses a modified ISO 15919) and as an optional scholarly field. They have no value for display.

**Which system matches signs and common use?** None exactly. Common Nepali practice looks like simplified NSD: ch/chh, sh for both श and ष, w more often than the table suggests, and the schwa deleted as pronounced. Two families of established exceptions sit on top of that:

- Generic-term spellings inherited from British/Indian English: *Chowk*, *Bazar*, *Ganj/Gunj*, *-pur*.
- Anglicised names: Kathmandu (not "Kathmadau"), Birgunj, Nepalgunj.

Indian practice is Hunterian without macrons, overlaid with legal renamings (Bengaluru 2014, Prayagraj 2018, Gurugram 2016) and surviving anglicisations (**renaming dates from general knowledge, unverified here**).

**OSM.** The OSM wiki Nepal page says: "The attribute name will always be in native name in Nepali unicode", with `name:en`/`name:ne` for other scripts and `name:alt` for more popular names. It also says: "Always use the official name when possible". The page links a "Tagging Guidelines for OSM Nepal" spreadsheet, which I did not read (**unverified**). Practice diverges from the wiki: an OSM diary post (n76, 17 Jan 2020) documents Kathmandu tagged `name=Kathmandu`, with local mappers putting romanised names in `name`. District pages such as Lalitpur say the opposite of the country page. A 2011 talk-np thread discussed how to tag chowks without reaching a settled rule. **Consequence: OSM `name` in Nepal may be Latin or Devanagari, and the importer must detect the script per tag.**

### 3.2 Evidence for generic-term spellings in OSM Nepal

I queried Geofabrik taginfo for `asia:nepal` (data until 2026-11-05T18:46Z). For each term, I summed the object counts of `name` values containing that term as a whole word. This is a usage signal, not an authority. Values include roads ("... Marg") and POIs.

| Term | Count | Variant counts |
|---|---|---|
| chowk | 758 | chok 476, chouk 0 |
| bazar | 389 | bajar 95, bazaar 79 |
| gaun | 377 | gaon 14 |
| tole | 468 | tol 457 |
| danda | 224 | dada 228 |
| bhanjyang | 140 | bhanjang 6, bhanjyan 16 |
| besi | 45 | beshi 12 |
| phedi | 61 | pedi 2 |
| pokhari | 691 | pokhri 5 |
| deurali | 275 | dewrali 0 |
| gadhi | 42 | garhi 0 |
| tal | 82 | taal 51 |
| dobhan | 20 | dovan 13 |
| chautari | 433 | chautara 160 (both are real words) |
| khola | 2,945 | |
| kot | 66 | kote 1 |
| kharka | 115 | |
| ghat | 67 | |
| marg | 2,875 | |

Nominatim (2026-11-06) resolves "Lahachowk" to features in Machhapuchchhre-04, Kaski, all spelled *Lahachowk*. This is consistent with the maintainer's ruling.

Two lessons follow. First, majority vote alone would choose *Dada* over *Danda* and only narrowly *Tole* over *Tol*, so the lexicon must be a curated decision with evidence attached, not a computed one. Second, Nepal and India differ (gaun/gaon, gadhi/garhi, ganj/gunj), so the lexicon is country-scoped.

### 3.3 Devanagari ⇄ Latin transliteration

**Linguistic points that matter for place names:**

- **Schwa (inherent a).** Hindi deletes word-final schwa and many medial ones (VC_CV contexts). Examples: विराटनगर → Viratnagar, not Viratanagar; ललितपुर → Lalitpur.
  - Rule-based Hindi deletion reaches more than 96% accuracy (Choudhury and Basu 2002, cited via Choudhury 2003). Narasimhan, Sproat and Kiraz (2004) combine Ohala's rule with morphology (89% by a later count). Arora, Gessler and Schneider (2020) train an orthography-only classifier that is state of the art for Hindi and transfers to Punjabi.
  - Nepali also deletes word-final schwa in most nouns, with exceptions: after conjuncts, in some verb forms, and in Sanskrit loans. NSD declines to give a rule.
  - Compounds matter: नगर, पुर and गाउँ are morphemes, and deletion applies per morpheme. काठमाडौं is read as Kathmandu, not "Kathamandu", which is why "Kathamandu" is a common user spelling. Splitting compounds with the lexicon before deletion fixes most errors.
- **Anusvara / chandrabindu.** Signage writes n before most consonants and m before labials (Kumbhakarna; Champa vs Chanpa). Final nasalised vowels (गाउँ, काठमाडौं) usually get n or nothing ("Gaun", "Kathmandu"). Nepali spelling varies between ं and ँ for the same word, so fold them for search.
- **Nukta.** Hindi uses ज़ (z), फ़ (f), ड़/ढ़ (ṛ/ṛh, giving "Garhi" and "Garhwal"). Nepali normally does not (NSD note 2), so the same morpheme gives "Gadhi" in Nepal and "Garhi" in India. Under NFC, U+0958–U+095F (क़ … य़) decompose to base + U+093C, but ऩ (U+0929) stays composed (verified with Python `unicodedata`, Unicode 17.0). Search folding must therefore handle both representations.
- **Conjuncts.** क्ष = ksh/x/kchh. Nepali pronounces it [cʰʌ], hence *Chhetrapati* and *Laxmi*/*Lakshmi*/*Lachhmi*. ज्ञ = gy/jn (Gyanendra/Jnanendra). त्र = tr. श्र = shr. Half forms and virama-less conjuncts must be handled before schwa deletion.
- **व as b/v/w.** Nepali often pronounces व as [b], which gives विराटनगर → *Biratnagar*. Hindi has v/w; NSD gives v/w.
- **श/ष/स.** Common use has sh/sh/s, and many writers use s for all three.
- **ऋ.** ri (NSD ṛi). Krishna, Rishikesh. The vowel sign ृ gives "ri" (कृष्णनगर → Krishnanagar).

**Libraries, measured on 2026-11-06 with the same nine Nepali names:**

- **ICU4C 79.1 `Devanagari-Latin`** (via `uconv`) is ISO 15919-like and keeps the schwa: काठमाडौं → `kāṭhamāḍauṁ`, लहाचोक → `lahācōka`, ललितपुर → `lalitapura`, भञ्ज्याङ → `bhañjyāṅa`. `Any-Latin; Latin-ASCII` gives `kathamadaum`, `lahacoka`, `bhanjyana`, `ga'um`. Its Inter-Indic pivot is the same trick IndicSOUNDEX uses.
- **`indic-transliteration` (sanscript), Python, MIT, 2.3.82.** IAST `lahācoka`, ITRANS `lahAchoka`. It is Sanskrit-oriented, with no schwa deletion. ITRANS→Devanagari of "kathmandu" gives कथ्मन्दु. Latin→Devanagari of informal spellings is unreliable.
- **Aksharamukha (AGPL-3.0, PyPI 2.3).** The public API "RomanColloquial" gives `kathamadaum lalitapura lahachoka bajara gaum danda bhanjyanga bemsi viratanagara`. A Hindi schwa post-option request returned HTTP 500 (**unverified** whether the option exists under that name). The AGPL licence rules out embedding it.
- **Rust crates (crates.io API, 2026-11-06).**
  - `vidyut-lipi` 0.2.0 (MIT, Sanskrit transliterator, ambuda-org)
  - `shlesha` 0.5.7 (MIT/Apache-2.0, ISO 15919 hub-and-spoke, includes ITRANS)
  - `lipilekhika` 1.1.2 (MIT, Brahmic scripts)
  - `IndicScriptSwap` 0.6.0 (2023)
  - `icu_experimental` 0.7.0, which has a `transliterate` module with `RuleCollection`/`Transliterator` (unstable API)

  None of them advertises Nepali or Hindi schwa deletion or signage-style output (README grep). `any_ascii` 0.3.3 (ISC) and `deunicode` 1.6.2 (BSD-3) do per-character ASCII and are fine for Latin diacritic stripping, but too crude for Devanagari.
- **Google Input Tools / Gboard Nepali transliteration** is a phonetic Latin→Devanagari input method with ranked suggestions. It is learned, not rule-based, and not available offline as a library (**unverified**, no primary documentation found). It shows that users type loose phonetic Latin (kathmandu, kathamandu, ktm).

**Conclusion.** We need our own readable profile. The library outputs are still useful as one more `alt` form in the index, and as a test oracle for the reversible ISO form.

### 3.4 Phonetic matching prior art

- **IndicSOUNDEX** (C. DiPersio, Amazon Alexa AI; WWW '21 Workshop on Multilingual Search) covers 9 Indic languages plus English. It works in three steps:
  1. Map each character by its offset in its Unicode block, so one table serves all Brahmic scripts, as ICU Inter-Indic does.
  2. Code the first letter as modified ISO 15919, with aspiration and diacritics stripped.
  3. Code the rest into 15 feature classes. Aspiration is merged, all nasals are merged, and vowels are dropped. Then remove h after a consonant, remove non-initial vowels and collapse repeated codes.

  Accuracy: Indic/Latin name pairs 95.71% overall (Hindi 96.49% on 9,280 pairs, Tamil 58.79%), ASR tokens 82%. It reports Chaware and Rao's monolingual results: custom Soundex 34% (Hindi) and 14% (Marathi); Q-gram 68% and 46%; "Indic-Phonetic" 76% and 38%. Lesson: an aggressive class mapping works well for Hindi and Latin, and badly where a script under-specifies voicing (Tamil). For Nepali and Hindi it is a good template.
- **Mlphon** (K. Manohar, A. R. Jayan, R. Rajan, *IEEE Access* 2022) is an FST-based Malayalam phonetic analyser built with SFST/HFST: G2P, P2G and syllabification, with 99% accuracy on its gold lexicon. It is relevant as a method (rules compiled to FSTs, gold lexicon evaluation), not as code, because it is Malayalam-only.
- **Soundex for Hindi.** Several smaller papers exist: Verma et al., Springer LNEE 2019/2020; Kant and Shrivastava, SSRN 2020 (Hindi in Roman script); Shah 2014 (Hindi/Gujarati); Jaisunder et al., GJEIS 2020. PyPI has `libindic-soundex` (Indic scripts plus English) and `indic-soundex` (Latin only). These were not read in full (**unverified** algorithms).
- **Double Metaphone / Beider-Morse.** Their English and European rule sets do not know aspirates, retroflexes or schwa. Double Metaphone also maps "th" to θ, which is wrong for थ/ठ. Beider-Morse picks rules by detected language and has no Indic rule set. Available in Rust as `rphonetic` 4.0.0 (Apache-2.0). Not suitable on their own.

### 3.5 Search techniques and how others do it

- **FST + Levenshtein automata.** `fst` 0.4.7 (MIT/Unlicense) stores sorted keys→u64 compactly. It supports prefix ranges and automaton intersection, and has an optional `levenshtein` feature. `levenshtein_automata` 0.2.1 (MIT, Tantivy) builds Schulz–Mihov DFAs much faster than the `fst` built-in. That it plugs into `fst` through the `Automaton` trait is **unverified**: it targets `tantivy-fst`, and an adapter may be needed. Cost grows with edit distance and dictionary size, so the common practice is d=0 for terms of 1–3 characters, d=1 for 4–7 and d=2 for 8 or more, often with the first character required to match.
- **SymSpell** (W. Garbe, MIT since v6.1, v6.7.3) uses symmetric delete: about 25 deletes cover all edit-distance-3 errors of a 5-letter word. It claims 0.033 ms per word at d=2. The cost is a large precomputed delete dictionary, which prefix indexing cuts by more than 90%. Rust ports exist: `symspell` 0.5.2 and `symspell_rs` 7.0.1 (both MIT). It is faster than automata for short queries but adds index size, which our "few MB" budget penalises.
- **n-gram / trigram indexes** give infix matching and robustness to segmentation ("laha chowk" vs "lahachowk"). They are larger and scored by Dice or Jaccard overlap. Useful as a fallback tier only.
- **BM25 / BM25F** (Robertson & Zaragoza 2009) helps multi-field, multi-token queries ("Lahachowk Kaski"). For a gazetteer, IDF over-rewards rare tokens. Generic words (chowk, tole, marg) are better down-weighted explicitly through the lexicon. `tantivy` 0.27.0 (MIT) gives BM25 off the shelf, but at a size and latency well above our budget.
- **Nominatim** (5.3.3 docs). The ICU tokenizer has three parts:
  - YAML-configured `normalization` (e.g. `:: lower ()`) and `transliteration` (ends in `:: Ascii ()`), which index every name in ASCII so Devanagari names become fuzzy-matchable in Latin.
  - Sanitizers, e.g. `tag-analyzer-by-language` for `name:xx`.
  - A `generic` analyser with variant lists: `road -> rd` adds a variant, `=>` replaces, `~` handles prefix/suffix decomposition (Hauptstrasse → Hauptstr / Haupt Str). Mutations are regex replacements, with a warning about combinatorial explosion.

  Importance comes from Wikipedia ranking values (a precomputed `wikimedia-importance.csv.gz`), with a static fallback by search rank. A secondary-importance raster serves as a tie-breaker. **Nominatim's variant lists are the closest prior art to our lexicon, and its `~` decomposition handles exactly the Lahachowk / Laha Chowk problem.**
- **Photon** (Apache-2.0) runs on OpenSearch 3.x. It offers search-as-you-type, multilingual search (dumps carry en/de/fr plus the local language), typo tolerance and location bias. Its README does not document the ranking (**unverified**).
- **Pelias** autocomplete documents a focus-point boost and popularity ordering without a focus point. The tokenisation and n-gram details are not on that page (**unverified**, known from code to use edge n-grams on Elasticsearch).
- **Google Places Autocomplete (New)** takes `languageCode`, which has a "small influence" on results, order and spelling correction. It also takes `includedRegionCodes` (up to 15), `locationBias`/`locationRestriction`, `inputOffset` (cursor position) and `origin`. Predictions may show "alternative names ... in a different language than expected", meaning it returns the name variant that matched the input. We should do the same and return the matched name plus the canonical name. Transliteration is not documented.
- **Apple Maps.** No primary documentation on transliterated search was found (**unverified**).
- **Importance signals available offline:**
  - admin level / place type
  - population (census)
  - Wikipedia pageviews (Wikimedia Analytics API, aggregated at build time)
  - Wikidata sitelink count
  - OSM tag richness (number of `name:*` tags, presence of `wikidata`)
  - number of child features

### 3.6 Unicode

- **Normalisation (UAX #15).** Store NFC. Note the Devanagari composition exclusions above. Use NFKC + case folding (`icu_normalizer` 2.4.0 / `icu_casemap` 2.4.0, Unicode-3.0 licence) only for the search form, which removes full-width forms and ligatures. Devanagari has no case.
- **Search-only Devanagari folds:**
  - remove nukta (U+093C)
  - ँ (U+0901) → ं (U+0902)
  - remove ZWJ/ZWNJ (U+200D/U+200C; Nepali uses ZWJ for eyelash र)
  - remove danda/double danda and avagraha
  - Devanagari digits → ASCII
  - fix mistyped independent vowels (अ + ा → आ, which input methods sometimes produce and which NFC does not merge; frequency **unverified**)
  - fold ऋ/रि only in the phonetic key
- **Segmentation (UAX #29, revision 49 for Unicode 18.0.0, 2026-09-01).** Extended grapheme clusters keep conjuncts together through rule GB9c and the Indic_Conjunct_Break property (introduced in Unicode 15.1, per my recollection, **unverified** here). Revision 49 changed GB9c again. Practical consequence: a user typing काठम is mid-cluster at times, and a prefix ending in virama (क्) must still match क्ष. **Prefix matching and edit distance must work on code points of the folded form, not on grapheme clusters.** Use grapheme clusters only for cursor and UI truncation. Word segmentation is whitespace-plus-punctuation for both scripts. Mixed-script tokens ("lahaचोक") are split at script boundaries (`unicode-script` 0.5.9).
- **Security (UTS #39 v18.0.0, 2026-08-27).** `skeleton()` is NFD, then drop default-ignorables, then map to prototypes, then NFD. Mixed-script detection uses augmented Script_Extensions. Latin + Devanagari is not "Highly Restrictive"; it can be "Moderately Restrictive" if Devanagari is a Recommended script (UAX #31 Table 5; **unverified** in the text read, but Devanagari is a modern recommended script in practice). For a gazetteer the risks are contributor-submitted names with Cyrillic/Greek homoglyphs ("Kathmаndu" with Cyrillic а) creating duplicates or spoofed entries. Rule: within a single token, reject Latin mixed with any other script. Skeleton-compare new names against existing ones to flag duplicates. `unicode-security` 0.1.3 (MIT/Apache-2.0) implements mixed-script and confusable detection.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Canonical = NSD/BGN-PCGN romanisation (Nepal) | Official, documented, BGN/PCGN-endorsed | Diacritics; no schwa rule; does not match signage (Kathmandu, Chowk) | Public document | 2011 agreement, PDF checked 2017 | Use for consonant choices only |
| Canonical = Hunterian / Survey of India (India) | Official national practice | Macrons; post-2006 status unverified; not reversible | Public | 1872, confirmed 2006 | Use, diacritics stripped, under entity overrides |
| ISO 15919 / IAST | Reversible, standard | Unreadable to the public; no schwa deletion | ISO (paid) / public | 2001 / 19th c. | Internal pivot and optional scholarly field only |
| Majority vote (OSM/GeoNames) | Automatic | Ties and wrong winners (dada > danda); not stable | ODbL / CC-BY | Live | Evidence input, not decision |
| Curated lexicon + entity overrides | Matches signage; contributor-editable; auditable | Needs curation and review | Ours (CC-BY or ODbL-compatible) | New | **Adopt** |
| ICU4C transliterators | Mature; Inter-Indic pivot; rule language | C++ dependency; ISO-style output; keeps schwa | Unicode-3.0 | ICU 79.1 | Test oracle only |
| ICU4X `icu_experimental::transliterate` | Pure Rust; custom rule collections | Unstable API; data coverage **unverified** | Unicode-3.0 | 0.7.0 | Watch; possible runtime for our rules |
| Aksharamukha | Many scripts; colloquial targets | AGPL; Python; no reliable schwa | AGPL-3.0 | 2.3 | Reject for embedding |
| indic-transliteration (sanscript) | Simple, MIT | Python; Sanskrit-oriented | MIT | 2.3.82 | Build-time oracle only |
| vidyut-lipi / shlesha / lipilekhika | Rust, permissive | Sanskrit focus; no Nepali/Hindi schwa or signage profile | MIT / MIT-Apache / MIT | 0.2.0 / 0.5.7 / 1.1.2 | Possible base for the ISO form; not for display |
| IndicSOUNDEX | Cross-script; measured ~96% Hindi/Latin | No public reference code found; drops all vowels (short place names collide) | Paper | 2021 | Adapt ideas |
| Double Metaphone / Beider-Morse (`rphonetic`) | Mature | Not Indic-aware | Apache-2.0 | 4.0.0 | Reject |
| `fst` + `levenshtein_automata` | Compact, µs lookups, prefix + fuzzy | Fuzzy d=2 cost on large sets; adapter **unverified** | MIT/Unlicense, MIT | 0.4.7 (2021), 0.2.1 (2021) | **Adopt** |
| SymSpell (`symspell`, `symspell_rs`) | Fastest typo lookup | Delete-dictionary size; no prefix-as-you-type | MIT | 0.5.2 / 7.0.1 | Benchmark as an alternative fuzzy tier |
| Trigram index | Infix and segmentation robustness | Size; weak ranking | n/a | n/a | Fallback tier, only if the golden set shows a need |
| `tantivy` BM25 | Full-featured | Size and latency above budget; overkill | MIT | 0.27.0 | Reject for runtime |
| Nominatim ICU variants model | Proven variant/abbreviation design | Server-side PostgreSQL | GPL-3.0 (Nominatim) | 5.3.3 | Copy the design, not the code |

## 5. What we build ourselves, and why

### 5.1 Canonical display spelling policy

Each place has one canonical Latin display name per country context. It is chosen by the first rule below that yields a value. Each step records its `source` and `evidence`.

1. **Entity override (authoritative).** This is a spelling published by the body responsible for that entity. Nepal: MoFAGA local-level names, Election Commission / Survey Department publications, and the national census. India: the Local Government Directory (lgdirectory.gov.in), Census of India, Survey of India, and gazette notifications of renamings. The override can contradict the lexicon. *Birgunj* and *Nepalgunj* keep "gunj" although the lexicon says "ganj". *Kukatpally* keeps "-pally". Legal renamings demote the old name to `former_official` (Bangalore → Bengaluru).
2. **Signage.** Photographed signs (contributor upload, Mapillary/Panoramax link) of the place's own name. Two independent signs beat one.
3. **Lexicon rendering.** Decompose the best source name into specific + generic using the country's lexicon (5.2). Replace the generic with its canonical form and keep the source's compounding: "Lahachok" → "Lahachowk", "Laha Chok" → "Laha Chowk". If sources disagree on compounding, prefer the joined form for settlements and the spaced form for natural features and streets. This follows the Survey of India practice above (**unverified** primary).
4. **Local usage majority.** Among OSM `name`/`name:en` values for this entity and its near-duplicates, take the majority after lexicon normalisation.
5. **GeoNames, then Wikidata English label / enwiki title.**
6. **Deterministic romanisation** of the official Devanagari name with the project's readable profile (5.4).

Display rules:

- No diacritics in the display name.
- Title case per token, keeping internal capitals only from step 1 sources.
- Hyphens are dropped unless the step 1 source has them.

Every non-winning spelling seen in any source becomes a stored `alt` name with its source, so search recall does not depend on this policy. A lexicon change re-renders affected names at build time and produces a reviewable diff.

### 5.2 Starter generic-terms lexicon (60 terms)

Canonical forms are proposals. "OSM" counts are from section 3.2. A term without evidence is marked *proposed* and needs a reviewer.

**Nepal (country `NP`, language `ne` unless noted)**

| # | Devanagari | Canonical | Variants (searchable, not displayed) | Meaning | Evidence / note |
|---|---|---|---|---|---|
| 1 | चोक, चौक | Chowk | Chok, Chouk, Chauk | square, crossroads | OSM 758 vs 476; maintainer ruling (Lahachowk) |
| 2 | बजार | Bazar | Bajar, Bazaar, Bajaar | market town | OSM 389/95/79 |
| 3 | गाउँ | Gaun | Gaon, Gau, Gaaun | village | OSM 377 vs 14 |
| 4 | टोल | Tole | Tol, Tola | neighbourhood | OSM 468 vs 457 (close); Kathmandu usage (Asan Tole) |
| 5 | डाँडा | Danda | Dada, Daanda, Dandaa | ridge | OSM 224 vs 228; nasal kept, following NSD n-before-ḍ |
| 6 | खोला | Khola | Kola, Kholaa | river, stream | OSM 2,945 |
| 7 | कोट | Kot | Kote, Kotte | fort, hill fort | OSM 66 vs 1 (Sarangkot, Nuwakot) |
| 8 | खर्क | Kharka | Kharkha, Kharaka | high pasture | OSM 115 |
| 9 | घाट | Ghat | Ghaat | riverbank steps | OSM 67 |
| 10 | बेंसी, बेसी | Besi | Beshi, Bensi, Beshee | valley floor | OSM 45 vs 12 |
| 11 | फेदी | Phedi | Pedi, Fedi, Phedee | foot of a hill | OSM 61 vs 2 |
| 12 | भञ्ज्याङ, भन्ज्याङ | Bhanjyang | Bhanjang, Bhanjyan, Bhanjyaang, Bhangyang | pass | OSM 140/6/16 |
| 13 | देउराली | Deurali | Dewrali, Deorali, Deuraali | pass with shrine | OSM 275 |
| 14 | पोखरी | Pokhari | Pokhri, Pokhree, Pokhary | pond | OSM 691 vs 5 |
| 15 | थोक | Thok | Thoke | (in toponyms) cluster, place | OSM 5; *proposed* |
| 16 | गढी | Gadhi | Garhi, Gadi, Ghadi | fort | OSM 42 vs 0 (Sindhuligadhi); India uses Garhi |
| 17 | गढ | Gadh | Garh, Gad | fort | Narayangadh; *proposed* |
| 18 | टार | Tar | Taar | river terrace | OSM 28 vs 9 |
| 19 | ताल | Tal | Taal | lake | OSM 82 vs 51 (Phewa Tal) |
| 20 | दह | Daha | Dah, Dahaa | lake, pool | OSM 61 |
| 21 | दोभान | Dobhan | Dovan, Doban, Dobhaan | confluence | OSM 20 vs 13 |
| 22 | चौर | Chaur | Chour, Chowr | meadow | OSM 121 |
| 23 | चौतारा | Chautara | Chautaara | resting platform (also a town) | OSM 160 |
| 24 | चौतारी | Chautari | Chautaari, Chautary | resting platform | OSM 433; distinct from #23 |
| 25 | बारी | Bari | Baari | garden, field | OSM 73 |
| 26 | पाटी | Pati | Paati | rest house | OSM 52 |
| 27 | ढुङ्गा | Dhunga | Dhungaa, Dunga | stone | OSM 47 |
| 28 | लेक | Lek | Lekh | highland | OSM "lekh" 11; *proposed*, Nepali spelling is लेक |
| 29 | हिमाल | Himal | Himaal | snow range | *proposed* |
| 30 | धारा | Dhara | Dhaara | water spout (Sundhara) | *proposed* |
| 31 | बहाल (Nepal Bhasa बहा:) | Bahal | Baha, Bahaal, Bahil | Newar monastery courtyard | *proposed*; endonym in `new` |
| 32 | थान, स्थान | Than | Thaan, Sthan, Asthan | shrine site | *proposed* |
| 33 | मार्ग | Marg | Marga, Maarg | road | OSM 2,875 (streets only) |
| 34 | पथ | Path | Patha | road | OSM 1,280 (streets only) |
| 35 | गल्ली | Galli | Gali | lane | *proposed* |
| 36 | नगर | Nagar | Nagara, Nagr | town | ICU/sanscript output "nagara" is a variant to fold |
| 37 | पुर | Pur | Poor, Pore, Pura (Latin only) | town | *-puri* and *-pura* are different morphemes (#55) |
| 38 | गञ्ज, गंज | Ganj | Gunj, Gunge | market town | Overridden per entity: Birgunj, Nepalgunj |
| 39 | गाउँपालिका | Gaunpalika | Gaupalika, Gaunpaalika | Rural Municipality | Admin suffix; English display "Rural Municipality" |
| 40 | नगरपालिका | Nagarpalika | Nagarpaalika | Municipality | Admin suffix |
| 41 | उपमहानगरपालिका | Upamahanagarpalika | Upa-mahanagarpalika | Sub-Metropolitan City | Admin suffix |
| 42 | महानगरपालिका | Mahanagarpalika | Mahanagarpaalika | Metropolitan City | Admin suffix |
| 43 | वडा | Ward | Wada, Woda, Wodaa | ward | Admin suffix |

**India (country `IN`, language `hi` unless noted)**

| # | Script | Canonical | Variants | Meaning | Evidence / note |
|---|---|---|---|---|---|
| 44 | चौक | Chowk | Chauk, Chok | square (Chandni Chowk) | *proposed*; same canonical as NP |
| 45 | बाज़ार, बाजार | Bazar | Bazaar, Bajar | market | Hunterian Bāzār; *proposed* |
| 46 | गाँव / Marathi गाव | Gaon | Gaun, Gaanv, Gav | village (Goregaon) | Differs from NP; *proposed* |
| 47 | गढ़ | Garh | Gadh, Gad | fort (Chandigarh) | Hindi ढ़ = ṛh |
| 48 | गढ़ी | Garhi | Gadhi | small fort | *proposed* |
| 49 | पुर | Pur | Poor, Pore | town (Jaipur, older Cawnpore) | |
| 50 | नगर | Nagar | Nagara (Kannada) | town | |
| 51 | गंज | Ganj | Gunj | market (Daryaganj) | Entity overrides apply (Tollygunge) |
| 52 | आबाद | Abad | Abaad, Bad | settled (Ahmedabad) | *proposed* |
| 53 | बाग़ | Bagh | Baag, Bag | garden | *proposed* |
| 54 | मंडी, मण्डी | Mandi | Mundi | market | *proposed* |
| 55 | पुरा / पुरी | Pura / Puri | (none; distinct morphemes) | quarter / town | Do not fold into Pur |
| 56 | खेड़ा | Khera | Kheda, Kheraa | hamlet | *proposed* |
| 57 | कलाँ / खुर्द | Kalan / Khurd | Kalaan / Khurad | greater / lesser (twin villages) | Qualifier: affects identity, never drop |
| 58 | Telugu పల్లి / Kannada ಹಳ್ಳಿ | Palli / Halli | Pally, Palle / Hally | village | Entity overrides (Kukatpally); languages te/kn |
| 59 | Marathi पेठ / Kannada ಪೇಟೆ | Peth / Pet | Peit, Pete | market quarter | mr/kn; *proposed* |
| 60 | वाडी | Wadi | Vadi, Wari, Vaadi | hamlet | mr; *proposed* |

Further candidates to add: टोला Tola/Tolla (Bihar, Bengal), सराय Sarai/Serai, घाटी Ghati, सागर Sagar, चौराहा Chauraha, Tamil -ur/-uru and -pattinam/-patnam, -kottai, -kunda, Odia/Bengali -pada/-para.

### 5.3 Phonetic key: algorithm draft (`pkey v0`)

**Purpose:** a candidate generator that maps spelling variants and both scripts to one short key. False merges are acceptable. Final ranking uses edit distance on the folded form plus importance. The key is computed per token and for the whole name with spaces removed, after lexicon canonicalisation, so Tol→Tole, Chok→Chowk and Laha Chowk = Lahachowk before phonetics.

**Key alphabet**

| Class | Members |
|---|---|
| Consonant classes | K G C J T D N P B R L S H Y M W |
| Vowel classes | A (a, ā, schwa); I (i, ī, e, ai, ei, ay); U (u, ū, o, au, ow, ou, oo) |

**Latin path, applied in order with longest match first at each step:**

1. NFKC, case fold, NFD, drop combining marks. Keep only a–z, split on everything else.
2. Lexicon: replace known variant generics with the canonical spelling (country-scoped). Expand abbreviations from the alias table.
3. Multigraphs → classes:
   - `ksh`, `kchh`, `x` → C
   - `chh`, `ch`, `c` → C (`c` before a/o/u stays K; `ck` → K)
   - `sh`, `ss`, `s` → S
   - `gy`, `jn`, `gn` before a vowel → G
   - `z`, `j` → J
   - `ph`, `f` → P
   - `bh`, `b`, `v`, `w` before a vowel → B
   - `w` or `u` after a vowel (`ow`, `ew`, `aw`) → U
   - `rh` → D (ड़/ढ़ written as "rh", so Garhi = Gadhi)
   - `kh`, `q`, `k` → K; `gh`, `g` → G; `th`, `t` → T; `dh`, `d` → D; `jh` → J
   - `ng` word-finally → N; `m`, `n` → N or M (see step 6)
   - remaining `h` after a consonant → dropped
4. Vowels → classes:
   - `aa`, `a` → A
   - `ee`, `ii`, `i`, `e`, `ai`, `ei`, `ay` (final) → I
   - `oo`, `uu`, `u`, `o`, `au`, `ou`, `ow` → U
   - `y` after a consonant → dropped (Bhanjyang = Bhanjang); `y` between vowels → Y
5. Drop every A that is not word-initial. This makes schwa retention or deletion irrelevant (Kathmandu = Kathamandu; Lalitpur = Lalitapura from ICU).
6. Nasals: a nasal (N or M) directly before a consonant class is dropped, which equates anusvara/chandrabindu with no nasal (Danda = Dada, Bensi = Besi). A word-final nasal after a vowel class is dropped (Gaun = Gau). Otherwise M → N, except before B or P.
7. Collapse runs of identical classes.
8. Empty key → fall back to the folded string.

**Devanagari path:**

1. NFC, then the search folds from 3.6.
2. Map each akshara to the same classes directly:
   - aspirates merged; retroflex = dental
   - श/ष/स → S; क्ष → C; ज्ञ → G; व and ब → B
   - ड़/ढ़ → D; ज़ → J; फ़ → P
   - ऋ/ृ → R + I
   - anusvara/chandrabindu → nasal marker, then step 6
   - inherent schwa → A; ा → A
   - ि/ी/े/ै → I; ु/ू/ो/ौ → U
3. Apply Latin steps 5–8 unchanged.

Because the non-initial A is dropped, no schwa-deletion model is needed for the key.

**Worked examples (hand-computed, to be turned into tests)**

| Input | Key | Notes |
|---|---|---|
| Lahachowk, Lahachok, Lahachouk, लहाचोक | LHCUK | |
| Kathmandu, Kathamandu, काठमाडौं | KTMDU | The pre-consonant nasal drop lets the exonym meet the endonym. Luck, not a design goal. |
| Bazar, Bajar, Bazaar, बजार, बाज़ार | BJR | |
| Gaun, Gaon, Gau, गाउँ | GU | Hindi गाँव gives GB under v0 rules. Known gap: needs a "final व after a vowel → U" rule, still to be tested |
| Bhanjyang, Bhanjang, भञ्ज्याङ | BJN | Short key, large bucket; ranking must cope |
| Deurali, Dewrali, Deorali, देउराली | DIURLI | |
| Biratnagar, Viratnagar, विराटनगर | BIRTGR | |
| Gadhi, Garhi, गढी, गढ़ी | GDI | |

**Spike S3 findings (2026-11-06, [results](../benches/s3-search.md)):**
1. Nasals are resolved *before* the non-initial A is dropped (pre-vowel nasals are real
   consonants).
2. Devanagari word-final schwa is deleted, so बजार and Bazar meet.
3. A written velar nasal (ङ, final "ng") stays unless a consonant follows (ढुङ्गा = Dhunga).
4. Each script run in a mixed word is classified on its own ("Anantaपुर" = Anantapur).
5. Measured: pair recall 0.789 on 303 lexicon pairs, false-merge rate 1e-4 on 34.5 million
   negative pairs, bucket p99 33.
6. A variant that drops word-final I/U raised pair recall to 0.855, but raised false merges by
   60% with no golden gain. It is rejected until S5's OSM pairs.
7. In ranking the key works only at low weight, below fuzzy and prefix.

**Known weaknesses.** Short keys (BJN, GU) produce huge buckets. Merging o/u, e/i and b/v/w is lossy by design. Tamil-style voicing ambiguity is out of scope for v0.

**Mitigation:** search the per-token key and the whole-name key together, and require that the specific (non-generic) part of the name contributes at least 2 consonant classes before a phonetic match counts.

### 5.4 Readable romanisation profile (Devanagari → Latin, for step 6 and indexing)

- **Basis:** NSD consonants, with sh for both श and ष, ksh for क्ष (variant "x" in alt), gy for ज्ञ, chh for छ, and ri for ऋ.
- **Nasals:** anusvara → n (m before labials). Chandrabindu → n word-finally after a vowel, otherwise dropped.
- **व:** v in Hindi. In Nepali, b word-initially when the word is on a curated list, otherwise w or v from the lexicon or entity data. Guessing is worse than an `alt` entry.
- **Schwa:** delete word-finally except after conjuncts. Delete medially at morpheme boundaries found by the lexicon (नगर, पुर, गाउँ, …). Then apply the Ohala-style VC_CV rule inside morphemes. Measure against a few hundred pairs taken from places that have both an official Latin and a Devanagari name.
- **Implementation:** ordered rule tables in data files, compiled into a Rust state machine. Evaluate running the same rules through `icu_experimental::transliterate`.

### 5.5 Lexicon file format (YAML schema)

One file per country, e.g. `data/lexicon/np.yaml`, validated by a JSON Schema in CI:

```yaml
schema: teigeodata/lexicon/v1
country: NP                     # ISO 3166-1 alpha-2; lexicon is country-scoped
updated: 2026-11-06
terms:
  - id: np.chowk                # stable id: <country>.<canonical-lowercase>
    canonical: Chowk            # display spelling (Latin, no diacritics, title case)
    native:                     # one or more native spellings, by language
      - { lang: ne, script: Deva, text: "चोक" }
      - { lang: ne, script: Deva, text: "चौक" }
    variants: [Chok, Chouk, Chauk]   # Latin spellings folded to canonical (search + render)
    kind: generic               # generic | admin_suffix | qualifier | street_type
    meaning: "square; crossroads"
    position: [suffix, standalone]   # where it may be recognised in a name
    compounding: preserve       # preserve | join | space  (how to render in a name)
    min_specific_len: 2         # chars required before a suffix match (avoid false hits)
    exceptions: []              # names where the variant must NOT be rewritten
    search_weight: 0.3          # down-weight in ranking (generic words are common)
    english: null               # display translation for admin suffixes, e.g. "Rural Municipality"
    status: accepted            # proposed | accepted | disputed | deprecated
    evidence:
      - kind: usage             # official | signage | survey_map | usage | literature | maintainer
        source: "Geofabrik taginfo asia:nepal, name values"
        detail: "chowk 758, chok 476, chouk 0 (data 2026-11-05)"
      - kind: maintainer
        source: "Maintainer ruling: Lahachowk, not Lahachok"
    reviewers: []               # handles of the reviewers who accepted it
```

**Rules for contributors:**

- A change to `canonical` needs at least one `official` or `signage` evidence item, or two independent `usage` items, plus one reviewer.
- `variants` may be added with lighter review, because they only add recall.
- `exceptions` and per-entity overrides live with the entity data, not in the lexicon.

### 5.6 Search architecture

**Build time (per country pack):**

1. For each name variant (all types), produce:
   - (a) the Latin folded form, after lexicon canonicalisation
   - (b) the Devanagari folded form
   - (c) the readable romanisation of every native-script name
   - (d) the `pkey` per token and for the whole name
   - (e) alias/abbreviation entries (`ktm`, `bkt`, colloquial names) from a curated `aliases.yaml`
2. Write one `fst::Map` with a 1-byte namespace prefix per form. The value is a u64 packing the posting-list offset (32 bits), the maximum score in the posting list (16 bits) and flags.
3. Write posting lists as delta-coded place ids with the name type and the matched-name id.
4. Write a precomputed top-k table (k = 10) for every 1- and 2-code-point prefix of form (a) and (b). Short prefixes are where FST traversal is most expensive.
5. Write the importance score per place, quantised to 16 bits:
   - place type / admin level prior
   - log10 population
   - log Wikipedia pageviews (12-month sum)
   - Wikidata sitelinks
   - OSM tag richness

**Query time:**

1. Normalise: split by script, fold as in 3.6, run the lexicon and alias lookup, compute the `pkey`.
2. Run the tiers in order and stop when k results reach a score floor:
   - exact on (a), (b), (e)
   - prefix on (a), (b), with the last token treated as a prefix
   - `pkey` exact
   - Levenshtein: d=1 for 4–7 code points, d=2 for 8 or more, first code point fixed
   - optional SymSpell tier, if benchmarks favour it
3. Combine multi-token queries ("Lahachowk Kaski") by intersecting the place candidates with ancestor names (hierarchy lookup), not with BM25.
4. Score, with weights tuned on the golden set:
   - match tier weight × name-type weight (official 1.0, local 0.95, alt 0.85, colloquial 0.8, former_official 0.7, historical 0.6)
   - plus importance
   - plus proximity, a Gaussian decay from an optional focus point, as in Pelias
   - minus a penalty for generic-only matches
5. Return the canonical name, the matched name and its type, as Google does.

**Budget to verify:** index size for Nepal ≤ 5 MB; p99 under 100 µs for warm queries on a 2020-class laptop core.

### 5.7 Golden-test plan

**Files:** `tests/golden/<country>.yaml`, one record per query:

```yaml
- id: np-0001
  query: "Lahachok"
  class: variant          # exact_latin | exact_native | variant | typo | prefix | abbreviation
                          # | historical | colloquial | mixed_script | with_context | ambiguous
  focus: null             # optional lat/lon for proximity tests
  expect: [np:place:lahachowk-kaski]   # relevant ids, graded below
  grades: { np:place:lahachowk-kaski: 3 }
  max_rank: 1             # hard assertion for CI
  source: maintainer
```

**Coverage:** at least 500 queries per country at first release, with each class at least 30.

**Sources:**

- maintainers and local contributors (variants, colloquial, abbreviations)
- OSM entities that have both `name:ne`/`name:hi` and `name:en` (exact_native and exact_latin pairs)
- GeoNames alternate names
- Wikidata labels and aliases
- historical names (Wikidata P1448 with end dates)
- synthetic typos: keyboard-adjacent substitution, transposition, deletion and doubled letters, generated from the canonical name with a fixed seed
- ambiguous names (Lalitpur NP vs Lalitpur IN; Pokhara vs Pokhari)

**Metrics, per class and overall:** recall@1, recall@5, MRR@10 and nDCG@10 (using the grades), plus latency p50/p99 and index bytes.

**Proposed gates (to be confirmed after the first baseline):**

| Metric | Gate |
|---|---|
| recall@1, exact classes | ≥ 0.95 |
| recall@5, variant / typo / mixed_script | ≥ 0.90 |
| MRR@10 overall | ≥ 0.85 |
| p99 latency | < 100 µs |

**Separate `pkey` test sets:**

- positive pairs: lexicon variants, OSM ne/en pairs, the table in 5.3
- negative pairs: distinct places with similar names in the same district

Metrics: pair recall, false-merge rate on negatives, and the bucket-size distribution (p50/p99 places per key). Reject a `pkey` change that raises the p99 bucket size by more than 20% without a matching gain in golden recall.

## 6. Impact on the plan

- **New ADR: Canonical Latin spelling policy.** The precedence in 5.1, with no diacritics in display, and every losing spelling kept as `alt`.
- **New ADR: Generic-terms lexicon.** The country-scoped YAML in 5.5, its review rules, and the boundary with per-entity overrides.
- **New ADR: Text normalisation.** NFC storage; search folds in 3.6; code-point prefix matching; UTS #39 build-time checks.
- **New ADR: Search index.** FST + Levenshtein, tiered matching, packed scores, top-k table for short prefixes. SymSpell to be benchmarked, BM25/tantivy rejected for runtime.
- **Architecture pages:** `name-model` gains name-level `source`, `evidence` and the `matched_name` return; `search-pipeline` describes the tiers; `transliteration` describes the readable profile and pkey v0.
- **Data:**
  - importer must detect the script of OSM `name` (Latin or Devanagari in Nepal)
  - collect MoFAGA and LGD names as override sources
  - add Wikipedia pageviews and Wikidata sitelinks to the build
- **Roadmap / gates:** the golden set (5.7) becomes the release gate for the Nepal pack. Building the 500-query set and the pkey pair sets is its own work item, before ranking work.

## 7. Risks and open items

- **Curation load and disputes.** Tole vs Tol and Danda vs Dada are close calls. Need a dispute process (`status: disputed` with both forms searchable) and named reviewers from Nepal and India.
- **Authority sources:** MoFAGA site did not respond to automated access on 2026-11-06. LGD licence and bulk-access terms not checked (**unverified**). Survey of India post-2006 practice **unverified**. OSM Nepal tagging spreadsheet not read.
- **Lexicon suffix matching** may rewrite non-generic endings (e.g. a specific ending in "-chok" that is not चोक). Mitigate with `min_specific_len`, `exceptions`, Devanagari cross-checks where a native name exists, and a build diff review.
- **pkey over-merging** for short names; the bucket metric is the guard.
- **Schwa model quality** for Nepali is unmeasured. Nepali-specific published rules were not found in this pass.
- **`levenshtein_automata` + `fst` integration** needs a spike (**unverified**). Fallback is `fst`'s own `levenshtein` feature or a custom bounded DFS over the FST.
- **Licences:** mixing ODbL (OSM) and CC-BY (GeoNames) names in one pack needs the data-licence ADR. Aksharamukha (AGPL) must not be linked.
- **India scale:** 640k+ villages and state languages beyond Hindi (Marathi, Bengali, Telugu, Tamil, Kannada…) need per-script folds. IndicSOUNDEX's block-offset trick extends `pkey` to other Brahmic scripts cheaply, though Tamil is known to be weak.
- **Open: whether to ship an `iso15919` field** (generated, cheap) for scholarly users.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| BGN/PCGN Romanization of Nepali | 2011 Agreement (validity check Oct 2017) | 2026-11-06 | assets.publishing.service.gov.uk |
| UK list of romanisation systems (intro) | June 2023 | 2026-11-06 | assets.publishing.service.gov.uk |
| UNGEGN WGRS Nepali status report | v4.0, Feb 2013 | 2026-11-06 | arhiiv.eki.ee/wgrs |
| UNGEGN WGRS Hindi status report | v4.0, Mar 2016 | 2026-11-06 | arhiiv.eki.ee/wgrs |
| UAX #29 Text Segmentation | Rev 49, Unicode 18.0.0 (2026-09-01) | 2026-11-06 | unicode.org |
| UTS #39 Security Mechanisms | 18.0.0 rev 34 (2026-08-27) | 2026-11-06 | unicode.org |
| ICU4C (`uconv`) | 79.1 | 2026-11-06 | local Homebrew |
| Python `unicodedata` | Unicode 17.0.0 | 2026-11-06 | local |
| indic-transliteration (PyPI) | 2.3.82, MIT | 2026-11-06 | pypi.org |
| aksharamukha (PyPI) | 2.3, AGPL-3.0 | 2026-11-06 | pypi.org |
| fst | 0.4.7 (2021-06-06), Unlicense/MIT | 2026-11-06 | crates.io |
| levenshtein_automata | 0.2.1 (2021-05-18), MIT | 2026-11-06 | crates.io |
| tantivy | 0.27.0, MIT | 2026-11-06 | crates.io |
| symspell / symspell_rs | 0.5.2 / 7.0.1, MIT | 2026-11-06 | crates.io |
| SymSpell (C#) | 6.7.3, MIT | 2026-11-06 | github.com/wolfgarbe/SymSpell |
| vidyut-lipi | 0.2.0, MIT | 2026-11-06 | crates.io |
| shlesha | 0.5.7, MIT OR Apache-2.0 | 2026-11-06 | crates.io |
| lipilekhika | 1.1.2, MIT | 2026-11-06 | crates.io |
| icu_experimental (transliterate) | 0.7.0, Unicode-3.0 | 2026-11-06 | crates.io, docs.rs |
| icu_normalizer / icu_casemap / icu_segmenter | 2.4.0, Unicode-3.0 | 2026-11-06 | crates.io |
| unicode-normalization | 0.1.26 | 2026-11-06 | crates.io |
| unicode-security | 0.1.3, MIT/Apache-2.0 | 2026-11-06 | crates.io |
| unicode-segmentation / unicode-script | 1.13.3 / 0.5.9 | 2026-11-06 | crates.io |
| rphonetic | 4.0.0, Apache-2.0 | 2026-11-06 | crates.io |
| any_ascii / deunicode | 0.3.3 ISC / 1.6.2 BSD-3 | 2026-11-06 | crates.io |
| Nominatim docs | 5.3.3 | 2026-11-06 | nominatim.org |
| Geofabrik taginfo asia:nepal | data until 2026-11-05T18:46Z | 2026-11-06 | taginfo.geofabrik.de |

## 9. Sources

- [Romanization of Nepali, BGN/PCGN 2011 Agreement](https://assets.publishing.service.gov.uk/media/5ab4e364e5274a1aa5933450/ROMANIZATION_OF_NEPALI.pdf), PCGN / UK Government, checked October 2017; accessed 2026-11-06. Mirror: [NGA GEOnames](https://geonames.nga.mil/geonames/GNSSearch/GNSDocs/romanization/ROMANIZATION_OF_NEPALI.pdf).
- [Introduction to Romanization Systems and Roman-script spelling conventions](https://assets.publishing.service.gov.uk/media/64996f80de86820013bc8dbc/Introduction_to_Romanization_Systems_and_Roman_script_spelling_conventions.pdf), PCGN, June 2023; accessed 2026-11-06.
- [Report on the current status of United Nations romanization systems: Nepali](https://arhiiv.eki.ee/wgrs/rom1_ne.htm), UNGEGN WGRS, v4.0, February 2013; accessed 2026-11-06.
- [Report on the current status of United Nations romanization systems: Hindi](https://arhiiv.eki.ee/wgrs/rom1_hi.htm), UNGEGN WGRS, v4.0, March 2016; accessed 2026-11-06.
- [UNGEGN WGRS: India / Hunterian system and Survey of India practice](https://arhiiv.eki.ee/wgrs/wgr06_5.htm), UNGEGN WGRS; accessed 2026-11-06.
- [UNGEGN Working Group on Romanization Systems](https://unstats.un.org/unsd/ungegn/working_groups/wg5.cshtml), United Nations; accessed 2026-11-06.
- [Transliteration System for Nepali Language](https://www.nepjol.info/index.php/NJG/article/view/23114), S. M. Shrestha, *Nepalese Journal on Geoinformatics* 11 (BS 2069); read via the [author's blog copy](https://sureshmanshrestha.blogspot.com/2020/10/transliteration-system-for-nepali.html), accessed 2026-11-06.
- [Survey of India place-name guidelines (summary gist)](https://gist.github.com/planemad/decdf608776726de1007), A. Ramachandran, GitHub; secondary; accessed 2026-11-06.
- [OSM wiki: Nepal](https://wiki.openstreetmap.org/wiki/Nepal), OpenStreetMap Wiki; accessed 2026-11-06.
- [OSM wiki: Names](https://wiki.openstreetmap.org/wiki/Names), OpenStreetMap Wiki; accessed 2026-11-06.
- [OSM diary: name tags in Nepal](https://www.openstreetmap.org/user/n76/diary/391926), n76, 2020-01-17; accessed 2026-11-06.
- [talk-np: Labeling chowks](https://lists.openstreetmap.org/pipermail/talk-np/2011-August/000007.html), OSM talk-np mailing list, August 2011; accessed 2026-11-06.
- [Geofabrik taginfo: Nepal](https://taginfo.geofabrik.de/asia:nepal/), Geofabrik, data 2026-11-05; queried 2026-11-06.
- [Nominatim search API](https://nominatim.openstreetmap.org/), OSMF, queried "Lahachowk" 2026-11-06.
- [IndicSOUNDEX Algorithm for Text Matching](https://cdn.amazon.science/86/4e/ebc49f01492d89307dbb45d0faf2/indicsoundex-algorithm-for-text-matching.pdf), C. DiPersio, Amazon Alexa AI, WWW '21 Workshop on Multilingual Search, 2021; accessed 2026-11-06.
- [Supervised Grapheme-to-Phoneme Conversion of Orthographic Schwas in Hindi and Punjabi](https://arxiv.org/abs/2004.10353), A. Arora, L. Gessler, N. Schneider, ACL 2020 / arXiv, April 2020; accessed 2026-11-06.
- [Rule-based grapheme to phoneme mapping for Hindi](https://www.cse.iitk.ac.in/users/cs671/2015/resources/choudhury-03_rule-based-grapheme-to-phoneme-mapping-hindi.pdf), M. Choudhury, 2003; cited, not read in full.
- [Schwa-deletion in Hindi text-to-speech synthesis](https://link.springer.com/doi/10.1007/s10772-009-9040-x), B. Narasimhan, R. Sproat, G. Kiraz, *Int. J. Speech Technology* 7(4), 2004; cited, not read.
- [FST based Malayalam Phonetic Analyser (Mlphon)](https://blog.smc.org.in/fst-based-malayalam-phonetic-analyser/amp/), K. Manohar, SMC; paper in *IEEE Access* 2022; accessed 2026-11-06.
- [Soundex Algorithm for Hindi Language Names](https://link.springer.com/chapter/10.1007/978-981-15-0372-6_22), Springer LNEE, 2020; abstract only.
- [UAX #29: Unicode Text Segmentation](https://www.unicode.org/reports/tr29/), Unicode Consortium, rev. 49, 2026-09-01.
- [UTS #39: Unicode Security Mechanisms](https://www.unicode.org/reports/tr39/), Unicode Consortium, 18.0.0, 2026-08-27.
- [UAX #15: Unicode Normalization Forms](https://www.unicode.org/reports/tr15/), Unicode Consortium; accessed 2026-11-06.
- [Nominatim: Tokenizers (ICU tokenizer)](https://nominatim.org/release-docs/latest/customize/Tokenizers/), Nominatim 5.3.3 manual; accessed 2026-11-06.
- [Nominatim: Importance](https://nominatim.org/release-docs/latest/customize/Importance/), Nominatim manual; accessed 2026-11-06.
- [Photon](https://github.com/komoot/photon), komoot, GitHub; accessed 2026-11-06.
- [Pelias autocomplete documentation](https://github.com/pelias/documentation/blob/master/autocomplete.md), Pelias, GitHub; accessed 2026-11-06.
- [Place Autocomplete (New)](https://developers.google.com/maps/documentation/places/web-service/place-autocomplete), Google Maps Platform; accessed 2026-11-06.
- [SymSpell](https://github.com/wolfgarbe/SymSpell), W. Garbe, GitHub, v6.7.3; accessed 2026-11-06.
- [Index 1,600,000,000 Keys with Automata and Rust](https://blog.burntsushi.net/transducers/), A. Gallant, 2015; background for `fst`.
- K. U. Schulz, S. Mihov, "Fast string correction with Levenshtein automata", *IJDAR* 5, 2002; background for `levenshtein_automata`.
- S. Robertson, H. Zaragoza, "The Probabilistic Relevance Framework: BM25 and Beyond", *Foundations and Trends in IR* 3(4), 2009.
- [crates.io API](https://crates.io/api/v1/crates/), queried 2026-11-06 for every crate in section 8.
- [Aksharamukha public API](https://aksharamukha-plugin.appspot.com/api/public), V. Vinodh; queried 2026-11-06.
- [Local Government Directory (India)](https://lgdirectory.gov.in/), Ministry of Panchayati Raj; reachable 2026-11-06, content not reviewed.
- [docs.rs: icu_experimental::transliterate](https://docs.rs/icu_experimental/0.7.0/icu_experimental/transliterate/index.html), ICU4X; accessed 2026-11-06.
