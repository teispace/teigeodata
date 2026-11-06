---
title: "ADR-0014: Names: English plus local; canonical spelling by lexicon; cross-script search"
status: accepted
updated: 2026-11-06
---

# ADR-0014: Names: English plus local; canonical spelling by lexicon; cross-script search

Status: accepted, 2026-11-06

## Context

Users search in Latin and Devanagari, with typos and variant spellings. Sources disagree on spelling (Lahachowk vs Lahachok).

## Decision

Each place has an English/Latin name, its official local-script name (Nepali; for India, Hindi and the state's official language), and **sourced** local endonyms, typed `official | local | alt | colloquial | historical | former_official`. The canonical Latin spelling is chosen by documented rules and a contributor-editable, sourced **generic-terms lexicon** (चोक → Chowk). Search works across scripts through a shared phonetic key, with prefix, fuzzy and abbreviation matching and ranking by importance and proximity. The policy and algorithms come from [names and search](../../02-research/notes/names-and-search.md).

Index design from [format, compression and indexing](../../02-research/notes/format-compression-indexing.md): an FST with ordinal outputs plus a range-maximum structure for top-k autocomplete (Hsu and Ottaviano), FSST name storage with per-script symbol tables, and Levenshtein automata (Schulz–Mihov) for fuzzy matching.

## Consequences

- A golden query set gates recall@1/@5.
- The lexicon lives in `data/lexicon/` with a schema and sources.

## Alternatives considered

Every world language (out of scope); search only in the query's script (fails users).

## Evidence

Maintainer direction, 2026-11-06.
