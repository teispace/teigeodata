---
title: "Reviewed names: a consumer's name review as a tier of the names chain"
status: research
updated: 2026-11-16
owner: maintainer's agents
revisit: 2027-05-16
---

# Reviewed names: a consumer's name review as a tier of the names chain

## 1. Question

Tithipatra read Nepal's names by eye against the government's lists (TG-14, their
`teigeodata-name-review.md`, 2026-11-12) and sent 357 corrections as a table keyed by our
stable IDs. How do reviewed corrections enter a build so they are applied, kept, and never go
stale, and how is the result measured? This feeds the TG-14 step in
[phase 4](../../07-planning/phases/phase-4.md) and the next data release.

## 2. Summary and recommendation

**Proposed (to be measured before it is accepted):**

- A per-country table, `data/countries/<cc>/reviewed-names.tsv` (P16: countries are data), one
  row per name: the place's stable ID, the name's language, the text it replaces (the guard),
  the corrected text, the fault, the confidence and the source. It is the **top tier** of the
  chain (§5.1 of the [Nepali names note](nepali-names.md)), above official lists, with its own
  origin code (`reviewed`, a new `NameRule` value: unknown codes already read as "other").
- **Keyed by stable ID, resolved through the registry.** A build has the ID registry
  (`--ids`); a reviewed ID that was retired follows its survivor, and a tombstone fails the
  build. A guard that no longer matches what the build would show fails it too, so a changed
  source is reviewed again instead of being overwritten silently.
- **What applies:** `sure` rows as written; `likely` rows whose source is a government list;
  `likely` rows on judgement alone, and every `uncertain` row, only after a second reading,
  recorded per row (`review` column). English rows change the canonical name only where a
  government list writes it so (Kapilvastu, Chitwan); otherwise the reviewed spelling is added
  as an `official` name and the canonical one is left alone.
- **Measured:** the rate of wrong names per kind before and after, against the review's own
  rows, in the build summary; the golden and G-golden-tithipatra gates must stay green.

## 3. Findings

- **The table (read 2026-11-16):** 357 rows, 293 Nepali and 64 English; 157 `sure`, 184
  `likely`, 16 `uncertain`; 185 rest on judgement, the rest on the Election Commission's
  lists, the constitution or the community dataset. By origin of the name replaced: official
  (CBS) 141, wikidata 64, wikidata `hi` on world places 49, donor 39, transliteration 34,
  unknown 22, geonames 8. 36 IDs carry more than one row. 49 rows are world places outside
  Nepal (25 in India).
- **IDs:** the registry (`forge/crates/forge/src/build/ids.rs`) maps each serial to its source
  keys and records retirements (`>N`) and tombstones; it is the bridge from a stable ID to the
  place a build makes.
- **Existing paths:** corrections (`data/countries/<cc>/corrections/*.yaml`) have `add-name`
  and `remove-name` with a guard and evidence per file; 357 single-file corrections would be
  the wrong shape for a table a consumer keeps sending. Tier 1 (`build/official.rs`) already
  places a name right after the canonical one so a reader asking for the language takes it
  first; the reviewed tier reuses that placement.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A reviewed-names table keyed by stable ID, a tier of the chain | One file per country, the consumer's own format, guards, applied in one place | A new origin code and a resolver through the registry | **Proposed** |
| One correction file per row (`add-name` + `remove-name`) | No new code | 357 files; source keys instead of IDs; no confidence | Rejected |
| Fix each source's text upstream (Wikidata, CBS) | Fixes it for everyone | Slow, and CBS publishes no corrections | Also do, where a source takes edits; not instead |

## 5. What we build ourselves, and why

The table's reader and its gate in the forge, the `reviewed` origin, and the summary's
before-and-after rates. Proved by tests: a row applied, a stale guard failing, a retired ID
following its survivor, a tombstone failing, and the second-reading rule.

## 6. Impact on the plan

The TG-14 step in phase 4; the schema's origin list; a data release with the corrections, its
name and rates sent to the consumer.

## 7. Risks and open items

- Rows on judgement are one reader's; a second reading is recorded before they apply.
- The world rows correct Hindi labels shown as Nepali stand-ins; they apply to `hi-Deva` names.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Tithipatra's review | 2026-11-12, 357 rows | 2026-11-16 | their `teigeodata-name-corrections.tsv` |

## 9. Sources

- Tithipatra, "teigeodata place names: Tithipatra's review" (2026-11-12) and its TSV.
- [Nepali names](nepali-names.md), §5.1 and §5.6 to §5.8.
- Election Commission of Nepal, local elections 2079 lookups (districts, local bodies, states).
- Constitution of Nepal 2072, Schedule 4.
