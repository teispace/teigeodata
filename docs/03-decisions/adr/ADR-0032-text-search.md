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

## Consequences

- SymSpell is benchmarked against Levenshtein automata; BM25/tantivy are not used at runtime.
- Building the golden set and the pkey pair sets is a work item before ranking work.
- Known gap: pkey v0 does not yet match Hindi गाँव to "Gaon"; it is fixed before the India wave.

## Alternatives considered

tantivy at runtime (too large for mobile and WASM); ICU transliteration alone (keeps the schwa); Soundex/Metaphone (built for English).

## Evidence

[Names and search](../../02-research/notes/names-and-search.md), 2026-11-06.
