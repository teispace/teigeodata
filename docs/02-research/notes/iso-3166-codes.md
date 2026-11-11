---
title: "ISO 3166 codes on countries and level-1 units"
status: research
updated: 2026-11-11
owner: maintainer's agents
revisit: 2026-12-15
---

# ISO 3166 codes on countries and level-1 units

## 1. Question

Consumer request TG-02 ([consumers/tithipatra.md](../../01-product/consumers/tithipatra.md),
[phase 4 §4.9b](../../07-planning/phases/phase-4.md#49b-consumer-requests)): every country and
level-1 admin unit carries its ISO 3166-1 alpha-2 or ISO 3166-2 code (`NP`, `NP-P3`; `IN`,
`IN-TN`), and the engine's `admin_at` and `ancestors` answers give them. Principle P16 holds:
no code names a country, and codes come from per-country data files with their source, never
from names.

This note answers:

1. The code format: how long a subdivision code can be, and whether it fits a `u32`.
2. The current codes of Nepal and India, with the ISO change dates.
3. The licence position of the codes, and where to source them per country.
4. How to store them in a pack (four options, with bytes for Nepal and the world baseline).
5. The engine API, the forge's input file and the gate.

## 2. Summary and recommendation

- **Format.** An ISO 3166-2 code is the alpha-2 code, a hyphen and 1 to 3 characters of
  `[A-Z0-9]`. Checked three ways (§3.1): the standard's description, Wikidata's constraint
  `[A-Z]{2}-[A-Z0-9]{1,3}`, and every regular subdivision ID in CLDR (none longer than 3). A full
  code packs into 28 bits of a `u32`, in an order that equals the codes' string order.
- **Store** the codes in a **new small section, `ISO_CODES` (kind 21), in the `admin`
  component** (option c): two packed columns, the sorted PLACES rows that have a code and each
  one's code, read with the existing `sorted::Sorted` lookup. About 0.6 kB on disk for Nepal and
  about 24 kB for the world baseline (estimates, §4; the build measures them). Unknown kinds are
  skipped by older readers, so no reader-major bump.
- **Store the full code, alpha-2 included.** The ID namespace is the allocating country, not the
  ISO code (ADR-0008), and under the Nepal view a unit's ISO prefix can differ from the country
  it hangs from (Gilgit-Baltistan `PK-GB` under India, D-156). Deriving alpha-2 would save about
  6 kB on the world baseline and be wrong.
- **API.** `Engine::iso_code(&Place) -> Result<Option<IsoCode>, Error>`, with
  `IsoCode` an inline, `Copy`, no-alloc type in `teigeodata-core` (`as_str`, `country`,
  `is_country`, `FromStr`); callers of `admin_at` and `ancestors` call it per entry. `Place`
  stays as it is, so points-only engines carry no ISO code (ADR-0015 tree-shaking). Optionally
  `Engine::by_iso(IsoCode) -> Result<Option<Place>, Error>`.
- **Input.** `data/countries/<cc>/iso3166.yaml` (schema `teigeodata/iso3166/v1`): the country's
  alpha-2, a `source` line with the ISO change date checked, the admin-source key of each unit
  and its code, `renamed` entries for stale codes in world sources, and `no_code` reasons.
  `split.yaml` stops repeating the codes and reads them from this file (one fact, one home).
- **Gate G-iso.** Every current level-1 unit of a pack country has a code or a stated reason;
  codes are well formed, unique and carry the country's prefix; the set equals CLDR's current
  subdivision list for the country; no code CLDR marks deprecated ships. The world part fails on
  a source's stale code with no `renamed` row. Each check is proven with a planted violation.
- **Sources.** Natural Earth 5.1.2's admin 1 is stale for both first countries (measured,
  §3.4): Nepal's 14 zones (deleted 2022-11-29) and India's pre-2023 `IN-TG`, `IN-CT`,
  `IN-UT`, `IN-OR`. CLDR's validity data (Unicode-3.0) marks those deprecated, so it is the
  check list; codes per unit come from the per-country file, cited to the ISO Online Browsing
  Platform (OBP) change and Wikidata P300 (CC0).
- **Unverified:** ISO's current terms of use of the codes (iso.org refused the fetch, §3.5);
  whether Overture's `region` values are current (D-155, main build).

## 3. Findings

### 3.1 The code format

- ISO 3166-2 codes have two parts: the ISO 3166-1 alpha-2 code and "a string of up to three
  alphanumeric characters", alphabetic, numeric or mixed, of constant or variable length within
  a country ([Wikipedia, ISO 3166-2](https://en.wikipedia.org/wiki/ISO_3166-2), citing the
  standard; accessed 2026-11-11).
- Wikidata's property P300 constrains values to `[A-Z]{2}-[A-Z0-9]{1,3}`
  ([P300](https://www.wikidata.org/wiki/Property:P300), accessed 2026-11-11).
- CLDR's subdivision validity list (`common/validity/subdivision.xml`, main branch, fetched
  2026-11-11) lists 3,484 regular IDs as written (ranges like `npp1~7` not expanded): 192 with a
  1-character subdivision part, 2,055 with 2, 1,237 with 3, none longer. The only longer IDs are
  CLDR's own `<cc>zzzz` "unknown" placeholders (257).
- Natural Earth 5.1.2 admin 1 (`ne_10m_admin_1_states_provinces`, 4,596 features, the
  pinned file in `data/cache`): every code without Natural Earth's `~` placeholder marker has at
  most 3 characters after the hyphen; the 188 `~` codes (`XK-X01~`, `-99-X16~`) are Natural
  Earth inventions, not ISO codes. 60 codes appear on more than one feature (a unit drawn in
  several pieces), 4,501 distinct.
- ISO 3166-1 reserves user-assigned alpha-2 codes (`AA`, `QM`–`QZ`, `XA`–`XZ`, `ZZ`); Kosovo's
  `XK` is one of these, used by the EU and others, not assigned by ISO. Natural Earth uses `-99`
  where no code applies.

**Packing into a `u32`.** Alpha letters are 1–26 (5 bits each); a subdivision character is 0
(none), 1–10 for `0`–`9`, 11–36 for `A`–`Z` (6 bits each):

```text
bits 27..23 alpha 1 | 22..18 alpha 2 | 17..12 sub 1 | 11..6 sub 2 | 5..0 sub 3
```

28 bits; never 0 (alpha 1 is at least 1), so the type can be a `NonZeroU32` in the format; a
country is a code with sub 1 = 0; and because 0 < digits < letters, as in ASCII, integer order
equals string order (`NP` < `NP-P1` < `NP-P7` < `NR`). Base-37 packing would take 26 bits and lose
the plain bit fields; not worth 2 bits.

### 3.2 ISO 3166-2:NP

Current codes, seven provinces: `NP-P1` Koshi, `NP-P2` Madhesh, `NP-P3` Bagmati, `NP-P4`
Gandaki, `NP-P5` Lumbini, `NP-P6` Karnali, `NP-P7` Sudurpashchim
([Wikipedia, ISO 3166-2:NP](https://en.wikipedia.org/wiki/ISO_3166-2:NP), which lists the OBP
changes; CLDR validity `npp1~7`; accessed 2026-11-11). The changes, from the OBP history on that
page:

| Date | Change |
|---|---|
| 2018-11-26 | `NP-P1` to `NP-P7` added (the provinces of the 2015 constitution) |
| 2020-11-24 | `NP-P3` and `NP-P7` renamed (Bagmati, Sudurpashchim) |
| 2021-11-25 | `NP-P5` spelling corrected |
| 2022-11-29 | Development regions `NP-1` to `NP-5` and the 14 zones (`NP-BA` … `NP-SE`) deleted; provinces renamed |
| 2023-11-23 | `NP-P1` renamed (Koshi) |

So the "2023 change" is a rename only; the P1–P7 codes date from 2018 and have been the only
first-level codes since 2022-11-29. These match `data/countries/np/split.yaml` (checked
2026-11-11) and the COD-AB P-codes `NP01`–`NP07` in the same order. CLDR keeps the 5 + 14
deleted codes as deprecated with no replacement (a region or zone does not map to one province).

### 3.3 ISO 3166-2:IN

36 codes: 28 states and 8 union territories, including `IN-TN` Tamil Nadu, `IN-DL` Delhi,
`IN-LA` Ladakh, `IN-DH` Dadra and Nagar Haveli and Daman and Diu
([Wikipedia, ISO 3166-2:IN](https://en.wikipedia.org/wiki/ISO_3166-2:IN), accessed
2026-11-11; CLDR's `IN` subgroup in `subdivisions.xml` lists the same 36). Changes that matter:

| Date | Change |
|---|---|
| 2014-10-30 | `IN-TG` Telangana added; Orissa renamed Odisha (code still `IN-OR`) |
| 2019-11-22 | `IN-JK` becomes a union territory; `IN-LA` Ladakh added |
| 2020-11-11 | `IN-DN` and `IN-DD` deleted; `IN-DH` added (merged union territory) |
| 2023-11-23 | Codes changed: `IN-OR` → `IN-OD`, `IN-CT` → `IN-CG`, `IN-TG` → `IN-TS`, `IN-UT` → `IN-UK` |

CLDR's validity list marks `inct`, `indd`, `indn`, `inor`, `intg`, `inut` deprecated (fetched
2026-11-11). Its `supplementalMetadata.xml` has the 2023 aliases only as comments with unknown
replacements (`intg` → `in?`), so CLDR tells us a code is stale but not what replaced it: the
`renamed` rows of §5.2 do that, cited to the ISO change.

### 3.4 What the pipeline's sources state today (measured 2026-11-11)

- **Natural Earth 5.1.2 admin 1** (lite world part, D-150): Nepal's 14 features carry the
  deleted zone codes (`NP-BA`, `NP-KA`, …) and India's carry `IN-TG`, `IN-CT`, `IN-UT`, `IN-OR`.
  Both are stale. Nepal's world rows are replaced by its slice (D-153), so Nepal's stale codes
  would not ship, but India's would. Natural Earth's `iso_a2` gives 241 distinct values.
- **Overture divisions** (main world part, D-155): `region` is stated as the ISO 3166-2 code
  (`forge/crates/forge/src/read/overture.rs`), from OSM's `ISO3166-2` tags. Whether those are
  current for India is **unverified**; the gate (§5.3) answers it on the first build.
- **COD-AB** (Nepal's pack admin source): P-codes, not ISO codes. The mapping is the data file.
- `docs/05-data/schema.md` lists `ext.iso3166_2` among cross-references; this note makes the
  code a first-class field instead, since it is identity data consumers key on.

### 3.5 Licence position

- The codes are short identifiers, used as facts. ISO sells the full ISO 3166 code collection
  and data services and lets anyone view the codes on the OBP. In 2003 ISO stated it would keep
  allowing free use of the ISO 3166, ISO 4217 and ISO 639 codes in commercial and other
  applications ([IAB statement, 2003-09-23](https://datatracker.ietf.org/doc/statement-iab-2003-09-23-isocodes/);
  [Unicode L2/03-340](https://www.unicode.org/L2/L2003/03340-iso-ref871.html)). Later
  third-party quotes of the OBP describe free use as narrower ("internal use and non-commercial
  purposes"). iso.org returned HTTP 403 to our fetch, so ISO's current wording is
  **unverified**; recording it is an open item for the maintainer (§7).
- What we ship is a code per unit, which every open geodata source carries: Wikidata P300
  (CC0), Natural Earth (public domain), CLDR (Unicode-3.0, a permissive licence with a notice),
  OSM and Overture (ODbL). We do not copy ISO's publication (names, remarks, lists of sources).
  Either build can carry the codes; cite the source per country in the data file and in the
  attribution page through `sources.lock` as for any source.

### 3.6 Where to source codes per country

| Source | Gives | Licence | Use |
|---|---|---|---|
| ISO OBP change history (via each `ISO 3166-2:<CC>` Wikipedia page) | current codes, change dates | facts; ISO terms unverified | the `source` line of each file: the change date checked |
| Wikidata P300 (and P297 for alpha-2) | code per item, start/end qualifiers | CC0 | fill a new country's file by the units' Wikidata IDs, which the pipeline already links |
| CLDR `subdivisions.xml` and `validity/subdivision.xml` | the current set per country; deprecated codes | Unicode-3.0 | the gate's check list (pinned in `sources.lock`) |
| Natural Earth admin 1 `iso_3166_2` / admin 0 `ISO_A2_EH` | code per world unit | public domain | lite world part, through `renamed` corrections |
| Overture divisions `country`, `region` | code per world unit | ODbL | main world part, through `renamed` corrections |

## 4. Options compared

Byte estimates use the bit-pack layout of `teigeodata-core::bitpack` (column header 8 B; per
128-value block a 4 B base, 4 B offset, 1 B width; payload `w × 16` B per block, partial blocks
padded) and the 80 B section-table entry (`layout::ENTRY_LEN`). Nepal: 8 coded rows (country
and 7 provinces) among about 97,000 places. World baseline: about 4,800 coded rows (about 250
countries and 4,571 units less fillers and units without a code) among 39,395 rows (D-153).
All are estimates until the build measures them.

| Option | Strengths | Weaknesses | Nepal | World baseline | Verdict |
|---|---|---|---|---|---|
| (a1) Sparse (row, code) as optional columns of HIERARCHY | No new kind; same component as the chain answers | `Hierarchy::open` refuses unknown columns, so every older reader fails on a new pack (reader-major bump); mixes tree structure with labels | ~0.6 kB | ~24 kB | Rejected: breaks old readers |
| (a2) The same columns as a COMPONENTS component | Built for sparse optional fields; presence as sorted rows exists | COMPONENTS is in the `extra` component with BLOBS (cold data): an admin client would download `extra` for 0.6 kB; the forge writes no COMPONENTS yet | ~0.6 kB + frame | ~24 kB | Rejected: wrong component |
| (a3) Optional columns of PLACES | Always loaded | Same old-reader break as (a1); points-only clients pay for it; PLACES columns are dense per row | ~0.6 kB | ~24 kB | Rejected |
| (b) A NAMES record of a new `NameType::Code` | No new kind | A code is not a name: SEARCH and the slice `NameRule` would key or drop it; FSST decode and a scan of the place's records per lookup; `Names::check` refuses an unknown type, so old readers fail; returns `&str` into a buffer | ~0.1 kB | ~58 kB (about 12 B a record) and more if keyed in SEARCH | Rejected |
| (c) New section `ISO_CODES` (kind 21) in `admin`: sorted rows + codes | Old readers skip unknown kinds; own Cargo feature path; O(log n) lookup through the existing `sorted::Sorted`; one code path for packs, slices and merges | One more section entry (80 B) per pack | ~0.68 kB (292 + 292 + 20 + 80) | ~24 kB (rows ~7 kB, codes ~17 kB) | **Recommended** |
| (d) Alpha-2 from the ID namespace, store only subdivision parts | Saves the 10 alpha bits per row | The namespace is the allocating country, not the ISO code (ADR-0008): stale after a split or a dependency's new code; world rows of disputed areas are `world`; a unit's ISO prefix can differ from its Nepal-view country (`PK-GB`, `PK-JK` under India; Taiwan's units under China) | ~0.6 kB | ~18 kB (saves ~6 kB, 0.1% of the 5.9 MB baseline) | Rejected: wrong by design |

The world codes column is wide (about 28 bits) because rows are in Hilbert order, so a block
mixes neighbouring countries. Sorting the section by code instead would narrow it but would need
a second index for row → code; 17 kB is 0.2% of Z1's 8 MB budget, so keep row order.

A raw-bytes column (codec 2) of `(row, code)` pairs would be 64 B for Nepal instead of about
600 B, but a second encoding is a second code path; 0.5 kB is not worth it (§11: one code path).

## 5. What we build ourselves, and why

### 5.1 Format and engine

- `teigeodata-format::iso_codes` (kind 21, `ISO_CODES`, component `admin`): a column table with
  `ROWS` (1, strictly increasing PLACES rows, the `sorted::Sorted` u32 form) and `CODE` (2, one
  packed code per row, §3.1). `open` checks equal lengths and that rows are below the PLACES
  count; `check` decodes every code and refuses an invalid one. A pack with no codes has no
  section. `extract::rows_with` and `merge` carry it, remapping rows as PLACE_TZ does.
- `teigeodata-core::IsoCode`: `#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]`,
  the text inline as `[u8; 6]` zero-padded (`NP`, `NP-P3`, `CN-TW`), so `as_str()` borrows it
  with no allocation and no buffer argument; `country() -> [u8; 2]`, `is_country()`,
  `FromStr` (case-insensitive, refuses malformed and user-assigned alpha-2), `from_packed(u32)`
  and `packed()` for the format.
- `Engine::iso_code(&self, place: &Place) -> Result<Option<IsoCode>, Error>`: the layer's
  section, a binary search on rows; behind the `admin` Cargo feature. Usage with the existing
  answers:

  ```rust
  engine.ancestors(&place, &mut chain)?;
  for p in &chain {
      if let Some(code) = engine.iso_code(p)? { /* "NP-P3", then "NP" */ }
  }
  ```

  Optionally `Engine::by_iso(code)` (a scan of at most a few thousand codes per layer, or a
  binary search if the codes are sorted per layer). Bindings expose the code as a string on the
  admin answer objects they already build.

### 5.2 Forge input: `data/countries/<cc>/iso3166.yaml`

```yaml
# ISO 3166 codes of Nepal's country row and units (principle P16: countries are data).
schema: teigeodata/iso3166/v1
country: NP                     # ISO 3166-1 alpha-2 of the country row; equals profile.yaml
# The ISO change last checked, and where.
source: "ISO 3166-2:NP, OBP change 2023-11-23, via https://en.wikipedia.org/wiki/ISO_3166-2:NP (checked 2026-11-11); Wikidata P300"
# Pack builds: admin-source key of each unit → its code (any level the standard codes).
units:
  "admin:NP01": NP-P1   # Koshi
  "admin:NP02": NP-P2   # Madhesh
  # ...
# World sources that state a code ISO has since changed: stale → current, with the change date.
renamed: []             # India: { from: IN-TG, to: IN-TS, on: 2023-11-23 }, and three more
# Units that have no ISO code, each with its reason.
no_code: {}
# Former units kept in the pack (FLAG_FORMER) may keep their deleted code, with its end date.
former: {}              # e.g. "admin:<zone key>": { code: NP-BA, until: 2022-11-29 }
```

`split.yaml` then keeps only `level` and reads each unit's pack address from this file.
`cargo xtask check-data` validates the schema.

### 5.3 Gate G-iso (forge and `check-data`)

1. Every code matches `^[A-Z]{2}(-[A-Z0-9]{1,3})?$`; no user-assigned alpha-2.
2. Every current (not `FLAG_FORMER`) level-1 unit of a pack country has a code or a `no_code`
   reason; every key in `units` names a unit of the admin source; codes are unique among
   current units; every subdivision code's prefix equals `country`.
3. Differential: a pack country's level-1 codes equal CLDR's subgroup for the country
   (`subdivisions.xml`, pinned), and no shipped code is deprecated in CLDR's validity list;
   differences fail unless listed in the file with a reason (a CLDR lag after an ISO change).
4. World part: every country row with an ISO-assigned alpha-2 has its code; a source's
   level-1 code that CLDR marks deprecated fails unless a `renamed` row maps it; disputed and
   filler pieces have none.
5. The baseline merge keeps every code of every part (the shipped-baseline test).

Proven by planting: a dropped code, `IN-TG` without its `renamed` row, a duplicate code, a
wrong prefix, a stale key, and a merge that drops the section; each must fail.

## 6. Impact on the plan

- Phase 4 §4.9b TG-02: format (kind 21, `admin` component), core type, engine accessor, forge
  stage and G-iso, in that order; a decision-log row records option (c) and the API.
- `docs/04-architecture/pack-format.md` and the component table in
  `crates/teigeodata-format/src/component.rs`: `admin` = HIERARCHY + `ISO_CODES`.
- `docs/05-data/schema.md`: the code becomes a field of the `admin` component; `ext.iso3166_2`
  is dropped from the cross-references.
- `data/countries/np/iso3166.yaml` and `data/countries/in/iso3166.yaml` (with the four 2023
  `renamed` rows); `split.yaml` loses its unit list.
- `data/sources.lock`: pin CLDR's `subdivisions.xml` and `validity/subdivision.xml`.
- `docs/09-guidelines` (adding a country): the file is required for a pack country.

## 7. Risks and open items

- ISO's current terms for the codes are **unverified** (iso.org 403): the maintainer records
  the wording, or a decision that codes are used as facts from CC0 and public-domain sources.
- ISO publishes changes in yearly batches near the end of November (2019-11-22, 2020-11-24,
  2021-11-25, 2022-11-29, 2023-11-23). A data review each December, and the CLDR differential,
  catch them; hence the revisit date.
- CLDR can lag ISO by a release; the gate allows a listed exception with a reason.
- Units that ISO codes below level 1 (France's departments, Spain's provinces) or several
  levels deep: the file maps any level, the gate requires level 1 only.
- Disputed areas under the Nepal view: which code a moved unit carries (its own, such as
  `PK-GB`, or none) is a data decision per file, recorded with the point of view.
- Whether Nepal's pack holds former zones and regions as `FLAG_FORMER` rows, and so whether
  `former` codes are worth shipping, is checked when the stage is built.
- Overture's `region` freshness for main is unverified until the first G-iso run.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Natural Earth admin 1 | 5.1.2 (`ne-admin1`, sha256 `22d0e3ad…`) | 2026-11-11 | `data/cache`, pinned in `data/sources.lock` |
| CLDR `validity/subdivision.xml`, `supplementalMetadata.xml`, `subdivisions.xml` | main branch | 2026-11-11 | github.com/unicode-org/cldr |
| Wikidata P300 constraint | `[A-Z]{2}-[A-Z0-9]{1,3}` | 2026-11-11 | wikidata.org |
| ISO 3166-2:NP latest change | OBP 2023-11-23 | 2026-11-11 | Wikipedia ISO 3166-2:NP |
| ISO 3166-2:IN latest change | OBP 2023-11-23 | 2026-11-11 | Wikipedia ISO 3166-2:IN |

## 9. Sources

- [ISO 3166-2](https://en.wikipedia.org/wiki/ISO_3166-2), Wikipedia, accessed 2026-11-11.
- [ISO 3166-2:NP](https://en.wikipedia.org/wiki/ISO_3166-2:NP), Wikipedia (OBP change history), accessed 2026-11-11.
- [ISO 3166-2:IN](https://en.wikipedia.org/wiki/ISO_3166-2:IN), Wikipedia (OBP change history), accessed 2026-11-11.
- [ISO 3166 country codes](https://www.iso.org/iso-3166-country-codes.html), ISO; refused the fetch (403), 2026-11-11.
- [Property P300, ISO 3166-2 code](https://www.wikidata.org/wiki/Property:P300), Wikidata, accessed 2026-11-11.
- [CLDR subdivisions.xml](https://github.com/unicode-org/cldr/blob/main/common/supplemental/subdivisions.xml), [validity/subdivision.xml](https://github.com/unicode-org/cldr/blob/main/common/validity/subdivision.xml) and [supplementalMetadata.xml](https://github.com/unicode-org/cldr/blob/main/common/supplemental/supplementalMetadata.xml), Unicode CLDR, main branch, accessed 2026-11-11.
- [IAB statement on ISO codes](https://datatracker.ietf.org/doc/statement-iab-2003-09-23-isocodes/), IETF, 2003-09-23.
- [ISO statement on the use of its codes](https://www.unicode.org/L2/L2003/03340-iso-ref871.html), Unicode L2/03-340, 2003.
- Natural Earth 1:10m admin 1, 5.1.2, public domain (pinned `ne-admin1`).
