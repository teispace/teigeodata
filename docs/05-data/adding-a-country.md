---
title: "Adding a country"
status: living
updated: 2026-11-08
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
                            such as indic-v0), romanise (the profiles in data/romanise/
                            that write names with no Latin spelling, such as ne-Deva)
  build.yaml                how `forge make <cc>` builds the packs: the admin and timezone
                            sources, the points sources with their readers in priority
                            order, the extent (the country's box with a 5 km halo) and
                            the building sources of the settlement core (D-095, D-096)
  lexicon.yaml              generic place-name terms and their canonical spellings; the
                            build respells variants in display names, but never the
                            `not_respelled` forms (other words, such as Pura)
  iso3166.yaml              required for a pack country: the country's ISO 3166-1
                            alpha-2 code, the source with the ISO change checked, each
                            admin unit's ISO 3166-2 code by its admin-source key, units
                            with no code and why, stale world-source codes (`renamed`)
                            and former units' deleted codes (D-197; gate G-iso)
  split.yaml                optional: the admin level a split build cuts at; the parts'
                            addresses come from iso3166.yaml (D-191, D-197)
  postal.yaml               postal schemes and their codes files (D-090, D-105)
  corrections/<id>.yaml     reviewed, guarded corrections (ADR-0036, D-117): add or
                            remove a name, move a place, set a population, or merge
                            two places (same-as)
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
4. **Lexicon:** start every term as `proposed`. Accept it only with evidence. A related form
   that is a different word in names goes in `not_respelled`, not `variants`. If the
   country's names are in a script with no profile in `data/romanise/`, write one
   (schema `teigeodata/romanise/v1`) and measure it on the country's paired names
   ([names-canonical](../02-research/notes/names-canonical.md)).
5. **Sources:** add each source to `data/sources.lock` with its licence class
   ([ADR-0030](../03-decisions/adr/ADR-0030-source-admission.md)), then list them in
   `build.yaml`; `forge make <cc> --release <v>` builds the main pack and `--lite` the lite one
   ([forge](../04-architecture/forge.md#run-every-stage-make)).
6. **Golden set:** generate the country's queries and add curated ones. A country ships only
   when its golden gate passes ([ADR-0032](../03-decisions/adr/ADR-0032-text-search.md)).
7. **Corrections:** fix what every source gets wrong with one file per fix,
   `corrections/<cc>-NNNN.yaml` (schema `teigeodata/correction/v1`). Target a source record
   (`geonames:1283240`); the guard is the field the fix was written against, printed by
   `forge correction-guard <source> <record> <name|point|population>`. Cite evidence with its
   licence class (CLEAN_ROOM.md rule 1; the lite build skips a fix whose evidence is not
   lite-clean). Each build writes `corrections-<build>.tsv`: applied, retired (the sources now
   agree) or stale (the guarded value changed); a stale fix older than the previous release
   fails the build (G-corr).
8. **Owners:** add a CODEOWNERS line for `/data/countries/<cc>/`.
