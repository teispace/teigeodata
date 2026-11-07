---
title: "Data schema v1 (draft)"
status: draft
updated: 2026-11-06
---

# Data schema v1 (draft)

The logical model every pack, export and API shares. The byte layout is in
[the pack format](../04-architecture/pack-format.md). Each rule links the decision it comes
from; this page gathers them in one place and does not override them.

**Conventions:**
- Every field is **nullable** unless marked *required*. A null means "unknown", never "zero".
- Optional fields are grouped into **components** a developer chooses at build or download
  time ([ADR-0015](../03-decisions/adr/ADR-0015-tree-shaking.md)). An absent component costs
  about one bit per row.
- Every field value carries **provenance** in the full build (source, source version, licence
  class, the rule that chose it; [ADR-0023](../03-decisions/adr/ADR-0023-provenance.md)).

## Identity

| Field | Type | Notes |
|---|---|---|
| `id` | u64, *required* | Stable, never reused ([ADR-0008](../03-decisions/adr/ADR-0008-stable-ids.md)). Text form `tgd:<cc>/<serial>`: the allocating country and the serial in Crockford base32, which round-trips (D-070). |
| `supersedes` | list of u64 | IDs merged into this one |
| `superseded_by` | u64 | Set on redirects; a tombstone has `superseded_by = null` and `status = retired` |
| `status` | enum, *required* | `current`, `redirect`, `retired` |
| `ext` | map scheme → string | Cross-references, never identity: `geonames`, `osm` (`n123`/`w123`/`r123`), `wikidata`, `gers`, `wof`, `cbs` (Nepal), `lgd` (India), `iso3166_2`, `unlocode` |

## Kind

| Field | Type | Notes |
|---|---|---|
| `kind` | enum, *required* | `country`, `admin` (with `admin_level`), `settlement`, `locality`, `natural`, `postal_area`, `timezone_area` |
| `admin_level` | u8 | 0 = country, then the national ladder. Nepal: 1 province (प्रदेश), 2 district (जिल्ला), 3 local level (पालिका), 4 ward (वडा) |
| `class` | dictionary code | Finer class: `city`, `town`, `village`, `hamlet`, `tole`, `metropolitan_city`, `rural_municipality`, `peak`, `pass`, `river`, … |
| `national_type` | string | The country's own type name as recorded (for example `Gaunpalika`) |

## Position and elevation ([ADR-0007](../03-decisions/adr/ADR-0007-accuracy-model.md))

| Field | Type | Notes |
|---|---|---|
| `point` | lat, lon, *required* | Consensus point (weighted geometric median). Stored on the 1e-6° grid for places; geometry uses 1e-7° ([ADR-0029](../03-decisions/adr/ADR-0029-coordinate-grid.md), D-035) |
| `uncertainty_m` | u16 | Horizontal σ, in metres |
| `label_point` | lat, lon | For areas: a point inside the polygon (polylabel) |
| `admin_centre` | lat, lon | Seat of an admin unit, where known: the point of its `seat` |
| `seat` | place ID | The populated place that is the unit's seat (OSM `admin_centre` role, Wikidata P36). A town and its eponymous unit stay two entities joined by this link (D-058) |
| `elevation_m` | i16 | Orthometric (EGM2008). The API also converts to ellipsoidal heights |
| `ele_uncertainty_m` | u16 | Vertical σ, including σ_h·tan(slope) ([ADR-0034](../03-decisions/adr/ADR-0034-elevation-method.md)) |
| `ele_method` | enum | `dem_consensus`, `surveyed`, `source_stated` |
| `ele_range` | i16 min, i16 max | Optional footprint statistics for areas |

## Hierarchy

| Field | Type | Notes |
|---|---|---|
| `parent` | u64 | The direct parent at the lowest covered level |
| `admin` | list of (level, id) | The full chain from the country down; derived from point-in-polygon, never from stated parents alone |
| `valid_from`, `valid_to` | date | For units that were created, merged or dissolved (for example Nepal's 2017 restructuring). Former units stay, with `status = retired` and links to their successors |
| `successors`, `predecessors` | list of u64 | Restructuring links (split and merge) |

## Names ([ADR-0014](../03-decisions/adr/ADR-0014-names-and-search.md), [ADR-0031](../03-decisions/adr/ADR-0031-canonical-spelling.md))

`name` is *required*: the canonical Latin spelling, chosen by the curated precedence. `names` is
a list of name records:

| Field | Type | Notes |
|---|---|---|
| `text` | string, NFC | As written |
| `lang` | BCP 47 language | `ne`, `hi`, `en`, `mai`, `new`, … |
| `script` | ISO 15924 | `Latn`, `Deva`, … |
| `type` | enum | `official`, `local`, `alt`, `colloquial`, `historical`, `former_official` |
| `valid_from`, `valid_to` | date | For historical and former names |
| `rule` | enum | Which precedence step chose a canonical name (official, signage, lexicon, osm_majority, geonames, wikidata, romanisation) |

Every losing spelling is kept as an `alt` name, so search still finds it.

## Boundaries

| Field | Type | Notes |
|---|---|---|
| `geometry` | polygon reference | Shared-arc topology. Levels of detail are chosen per pack ([format](../04-architecture/pack-format.md) sections ARCS and POLYGONS) |
| `bbox` | 4 × i32 | 1e-7° grid |
| `area_km2` | f32 | Computed on the ellipsoid |
| `perspective` | set of perspective codes | Which boundary views include this geometry. The default is `np` ([ADR-0006](../03-decisions/adr/ADR-0006-perspective.md)); others (`in`, `de-facto`) are added as data |

## Timezone and postal codes ([ADR-0022](../03-decisions/adr/ADR-0022-timezone-and-postal.md), [ADR-0033](../03-decisions/adr/ADR-0033-timezone-data.md))

| Field | Type | Notes |
|---|---|---|
| `timezone` | IANA zone ID | Exact point lookup from the region index. Where source zones overlap, the precedence rule applies (Q-013) |
| `postal` | list of postal records | Each record has `code`, `scheme` (for example `np`, `np-1991`, `in-pin`; D-090), `kind` (`area`, `post_office`, `point`), `valid_from`, `valid_to` and `redirect_to`. Current and former codes coexist |
| `digipin` | string | India only: computed from the point, never stored as a source fact |

## Population and other optional data

| Field | Type | Notes |
|---|---|---|
| `population` | u32 | With `population_year` and `population_source`. Census values beat estimates |
| `population_kind` | enum | `census`, `estimate`, `modelled` |
| `importance` | u8 | Ranking score for search (population, admin level, links); documented formula |
| `wikipedia` | map lang → title | Optional |

## Provenance (component `provenance`, on in the full build)

For each field: `source` (adapter ID), `source_version`, `licence_class` (as in
[CLEAN_ROOM.md](../../CLEAN_ROOM.md)), `rule`, and an optional `correction` ID
([ADR-0036](../03-decisions/adr/ADR-0036-corrections-as-data.md)).

## Components

| Component | Fields | Default |
|---|---|---|
| `core` | identity, kind, point, name, parent | always |
| `admin` | admin chain, validity, restructuring links | on |
| `names` | the name list | on |
| `elevation` | elevation fields | on |
| `boundaries` | geometry per level of detail | off; chosen per LOD |
| `timezone` | timezone | on |
| `postal` | postal records | on where data exists |
| `population` | population fields, importance | on |
| `external` | `ext` cross-references, Wikipedia | off |
| `provenance` | per-field provenance | off; on in the full build |

## Open items

- The class dictionary: drafted per country during Phase 3 from source mappings.
- The importance formula: set and tested against the golden set (spike S3).
