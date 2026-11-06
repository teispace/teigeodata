---
title: "Nepal (NP)"
status: draft
updated: 2026-11-06
---

# Nepal (NP)

The first country. Research: [Nepal sources](../../02-research/notes/nepal-sources.md). Licence
rules: [ADR-0030](../../03-decisions/adr/ADR-0030-source-admission.md).

## Levels

| Level | Count | Key | Main source | Lite source |
|---|---:|---|---|---|
| Country | 1 | ISO `NP` | OSM relation joined with the disputed relation 13559363 (`claimed_by=NP`) | COD-AB (claim area to verify, Q-010) |
| Province (प्रदेश) | 7 | CBS code | OCHA COD-AB v02 | COD-AB v02 |
| District (जिल्ला) | 77 | CBS code | COD-AB v02 | COD-AB v02 |
| Local level (पालिका) | 753 (+22 protected-area units) | CBS code (e.g. Kathmandu Metropolitan City `30608`) | COD-AB v02 | COD-AB v02 |
| Ward (वडा) | 6,743 (OSM has 6,744 `admin_level=9` relations) | CBS code + ward number | OSM `admin_level=9` | codes only, no geometry |
| Settlement / tole | tens of thousands | our stable ID | conflated OSM, GeoNames, Wikidata | GeoNames, Wikidata |

## Facts

- **Perspective:** Nepal's official map (2020), including Kalapani, Lipulekh and Limpiyadhura
  ([ADR-0006](../../03-decisions/adr/ADR-0006-perspective.md)).
- **Timezone:** `Asia/Kathmandu`: +05:30 from 1920, +05:45 from 1986 (tzdb).
- **Names:** Nepali (Devanagari) official names come with the CBS codes; canonical Latin
  spelling follows [ADR-0031](../../03-decisions/adr/ADR-0031-canonical-spelling.md) and
  [the lexicon](../../../data/lexicon/np.yaml). Endonyms in other local languages are sparse in
  open sources.
- **Postal codes:** unresolved ([Q-001](../../07-planning/open-questions.md)).
- **Elevation ground truth:** Survey Department trig points are sold, not open; validation uses
  ICESat-2 ([ADR-0034](../../03-decisions/adr/ADR-0034-elevation-method.md)).

## Excluded sources

Kosi (CC BY-NC-SA), nepalboundariespy (unknown data source), geoBoundaries NPL ADM2 (2006
districts), geoBoundaries NPL ADM3 (CC BY-SA), Open Data Nepal boundaries (CC BY-SA).
