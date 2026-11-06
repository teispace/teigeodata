---
title: "Adding a country"
status: living
updated: 2026-11-06
---

# Adding a country

Countries are data, not code ([principle P16](../00-vision/principles.md)). The same schema,
algorithms, checks and tools serve every country. Adding one means adding a folder and its
research, never changing core code. If a country seems to need code, that need is a missing
general feature: design it for every country and record it in the decision log.

## The folder

```
data/countries/<cc>/        lower-case ISO 3166-1 alpha-2
  profile.yaml              required: languages, scripts, admin ladder, unit types,
                            abbreviations, droppable suffixes, source codes;
                            optional: wikidata (the country item, for per-country
                            queries), phonetic_key (an algorithm of teigeodata-names,
                            such as indic-v0)
  lexicon.yaml              generic place-name terms and their canonical spellings
  corrections/<id>.yaml     reviewed corrections (ADR-0036)
  golden-curated.yaml       hand-written search queries for the golden set
```

`cargo xtask check-data` validates every folder against the same rules, in CI and in the
pre-commit hook.

## Steps

1. **Research note:** sources, licences, the admin structure, scripts and languages, postal and
   timezone facts, and disputed areas, following
   [the Nepal note](../02-research/notes/nepal-sources.md) as the model (AGENTS.md §4).
2. **Country page:** `docs/05-data/countries/<cc>.md`, like [Nepal](countries/np.md).
3. **Profile:** every value needs a source. Unit-type counts and source codes are measured from
   the data, as for Nepal's COD-AB P-codes.
4. **Lexicon:** start every term as `proposed`. Accept it only with evidence.
5. **Sources:** add each source to `data/sources.lock` with its licence class
   ([ADR-0030](../03-decisions/adr/ADR-0030-source-admission.md)).
6. **Golden set:** generate the country's queries and add curated ones. A country ships only
   when its golden gate passes ([ADR-0032](../03-decisions/adr/ADR-0032-text-search.md)).
7. **Owners:** add a CODEOWNERS line for `/data/countries/<cc>/`.
