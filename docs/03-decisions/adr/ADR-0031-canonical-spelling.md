---
title: "ADR-0031: Canonical Latin spelling: curated precedence and a per-country lexicon"
status: accepted
updated: 2026-11-06
---

# ADR-0031: Canonical Latin spelling: curated precedence and a per-country lexicon

Status: accepted, 2026-11-06. Refines [ADR-0014](ADR-0014-names-and-search.md).

## Context

No published romanisation standard matches how places are spelled on signs: Nepal's Survey Department system and India's Hunterian system both use diacritics, and existing transliterators keep the schwa (ICU gives `lahācōka`). Source votes mislead: in OSM Nepal, Chowk beats chok 758 to 476, but dada narrowly beats danda.

## Decision

The canonical display spelling is a **curated convention without diacritics**, chosen per place in this order:

1. an official spelling recorded for that entity (Nepal: the federal affairs ministry's local-level names; India: the Local Government Directory or Survey of India);
2. documented signage;
3. the **generic-terms lexicon** applied to the source name (चोक → **Chowk**, so Lahachok → Lahachowk);
4. the OpenStreetMap majority in the area;
5. GeoNames, then Wikidata English labels;
6. deterministic romanisation of the Devanagari name with our readable profile (schwa deletion, survey-style consonants).

**Every losing spelling is kept as an `alt` name**, so search still finds it. The lexicon is YAML keyed by country (Nepal uses Gaun and Gadhi; India uses Gaon and Garhi), each entry with its sources and a review rule; per-entity overrides are separate from generic terms.

## Consequences

- `data/countries/<cc>/lexicon.yaml` with a schema checked by `cargo xtask check-data`; a starter list of 60 Nepal and India terms comes from the research note.
- Names carry `source` and `evidence`; search returns the `matched_name`.
- The forge detects the script of OSM `name` (Latin or Devanagari in Nepal) and collects official name registers as override sources.

## Alternatives considered

A single transliteration standard (does not match use); majority vote (wrong for danda/dada); keeping diacritics in display (hard to type, not what signs show).

## Evidence

[Names and search](../../02-research/notes/names-and-search.md), 2026-11-06.
