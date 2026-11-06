---
title: "ADR-0032: Text normalisation and the search index"
status: accepted
updated: 2026-11-06
---

# ADR-0032: Text normalisation and the search index

Status: accepted, 2026-11-06. Refines [ADR-0014](ADR-0014-names-and-search.md).

## Context

Search must find a place from Latin or Devanagari input, with typos, variants and abbreviations, offline, in under 100 µs, from a few MB.

## Decision

- **Normalisation:** store NFC. Search folding: NFKC plus case folding for Latin; for Devanagari, fold nukta, chandrabindu → anusvara and ZWJ/ZWNJ (search only). Prefix matching is on code points. Mixed-script contributor names are checked against UTS #39 at build time.
- **Transliteration:** our own small rule tables for a readable Devanagari → Latin profile; ICU4X-compatible rule data where possible.
- **Phonetic key (pkey):** our own ordered folding in the spirit of IndicSOUNDEX. Every non-initial *a*-class vowel is dropped, which makes schwa deletion irrelevant; o/u and e/i merge. It produces the same key from either script. The key generates candidates and never ranks.
- **Index:** one `fst` map per normalised form (Latin-folded, Devanagari-folded, pkey, alias); Levenshtein automata with an edit budget by query length; top-k prefix completion from scores packed in the FST value plus a precomputed table for 1–2 character prefixes.
- **Ranking:** match quality, then importance (admin level, log population, Wikipedia pageviews, OSM tag richness), then proximity to a focus point, then name type.
- **Golden set:** at least 500 graded queries per country in 11 query classes; recall@1, recall@5, MRR@10 per class, latency p50/p99 and index size; a release gate for each country's pack.

Refined 2026-11-06 for world data (principle P16, D-051):

- **Folds are per Unicode script, never per country.** Optional accents are dropped only in Latin, Greek and Cyrillic. Elsewhere, marks that carry meaning are kept: Indic vowel signs and virama, Arabic harakat, Thai vowels and tone marks. Brahmic folds (nukta dropped, chandrabindu → anusvara) apply to every Brahmic script. The production `names` crate derives these sets from the Unicode properties (`Indic_Syllabic_Category` = Nukta and Bindu) instead of code-point lists, so new scripts such as Newa are covered.
- **Country rules are data.** Unit types, abbreviations, droppable suffixes and native scripts come from the country profile; the golden generator and the search index read them.
- **Golden sets are per country.** The generated `tests/golden/<cc>.yaml` and the curated `data/countries/<cc>/golden-curated.yaml` share the schema `teigeodata/golden/v0`. Keys are source keys until stable IDs exist. Nepal v0 has 512 queries (500 generated, 12 curated).

Refined 2026-11-06 by spike S3 ([results](../../02-research/benches/s3-search.md), D-052):

- **Fuzzy is our own Damerau (OSA) automaton** over the FST, one edit, anchored on the first letter, with an unanchored pass when nothing exact, loose or within one edit matched. The `fst` crate's Levenshtein DFA cost about 10× as much per query. Two edits are off; they bought no recall.
- **One additive score for every country:** a match bonus plus importance (exact 60, loose 55, prefix 30, one edit 25, phonetic 5, unit-type match +20). A parent named in the query ("Name, Parent") outranks everything else. Ranking tiers strictly (exact always before fuzzy) failed prefix queries, and a high phonetic bonus caused false merges.
- **The phonetic key is a low-weight safety net.** `indic-v0` stays, with the S3 fixes (nasals before A-dropping, word-final schwa deletion, ङ kept unless a consonant follows, script runs classified separately). `indic-v1` (drop word-final I/U) is re-tested on OSM pairs in S5.
- **Recall gates met on Nepal:** recall@1 0.953, recall@5 0.986, MRR@10 0.969, 3.6 MB index. p99 is 78–116 µs, depending on machine load, so the 100 µs gate needs a faster automaton in Phase 2. The remaining misses are data (aliases, conflation), not search.

## Consequences

- SymSpell is benchmarked against Levenshtein automata; BM25/tantivy are not used at runtime.
- Building the golden set and the pkey pair sets is a work item before ranking work (done for Nepal in S3).
- Known gap: pkey v0 does not yet match Hindi गाँव to "Gaon"; it is fixed before the India wave.

## Alternatives considered

tantivy at runtime (too large for mobile and WASM); ICU transliteration alone (keeps the schwa); Soundex/Metaphone (built for English).

## Evidence

[Names and search](../../02-research/notes/names-and-search.md), 2026-11-06.
