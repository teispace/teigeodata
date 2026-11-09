---
title: "Timezones in the lite build"
status: accepted
updated: 2026-11-09
owner: forge
revisit: 2027-05-01
---

# Timezones in the lite build

## 1. Question

The lite build (CC BY 4.0) may not read timezone-boundary-builder (TBB). TBB is ODbL because
it is drawn from OpenStreetMap ([CLEAN_ROOM.md](../../../CLEAN_ROOM.md) rule 2). The Nepal lite
pack therefore had no `timezone` or `timezone-index` component (summary of 2026-11-09:
"timezones none"). Which clean-room source gives it timezones, and how exact is the result?
This feeds phase 3.7, "the lite build from clean-room sources only".

## 2. Summary and recommendation

Use the **IANA tzdb** release with the same name as the main build's TBB release, pinned as a
source (`tzdb-2026f`). Its files are in the public domain. Use it as follows:

- A country with **one row in `zone.tab`** gets that zone over its own admin-0 outline, from
  the build's admin source (COD-AB for Nepal, CC BY-IGO). This is exact by construction
  wherever the outline is: 216 of the 247 countries in `zone.tab` 2026f have one row.
- `zone.tab` gives the country-specific ID, which is what TBB returns (`Europe/Amsterdam`, not
  `zone1970.tab`'s shared `Europe/Brussels`). Main and lite therefore name a place's zone the
  same way.
- A country with several rows needs a per-country table of admin units to zones
  (`data/countries/<cc>/timezones.yaml`, with sources). That table does not exist yet, and the
  build refuses such a country with a message naming the file. India has one row
  (`Asia/Kolkata`), so the rollout through South Asia needs no table.

The recipe's `timezones` becomes a priority list, as `dem` did (D-132):
`[tbb-2026f, tzdb-2026f]`. Each build takes the first source its licence gate allows: main
takes TBB, lite takes tzdb.

**Independent check (follow-up):** GeoNames records carry a `timezone` column, and GeoNames is
CC BY 4.0, so lite may read it. Agreement between that column and the pack's `PLACE_TZ` is a
gate to add once the reader keeps the column.

**Measured** (Nepal lite, 2026-11-09):

- 94,733 of 94,761 places have `Asia/Kathmandu`. The other 28 are the places outside every
  COD-AB unit (G-pip).
- `PLACE_TZ` is 7,056 B (291 B on the wire); REGION is 543,196 B (368,538 B on the wire).
  Main's TBB REGION is 2,007,656 B (1,605,830 B) for six areas.
- The golden gate is unchanged, and the main pack's sections are byte-identical.

## 3. Findings

- **tzdb 2026f** (`tzdata2026f.tar.gz`, 483,310 B, SHA-256 `01b4a91c…`, fetched 2026-11-09).
  - Its `LICENSE` states that all data files are in the public domain.
  - `zone.tab` lists one row per country and zone, with an ISO 6709 point.
  - Counting countries by their rows: 216 have one row and 31 have several. In
    `zone1970.tab`, where a row may list several countries, the count is 214 and 33.
- Nepal: `NP +2743+08519 Asia/Kathmandu`, its only row. Nepal Standard Time is UTC+05:45.
- `zone.tab` "is not intended to take or endorse any position on legal or territorial
  claims" (its header). The outline therefore decides the area, and that is the build's admin
  source. COD-AB excludes Nepal's claimed area (Q-015), so the lite zone does too, exactly as
  lite's admin units do.
- [timezone-and-elevation](timezone-and-elevation.md) §6 already recommended this
  "country plus admin" design for lite. It is the approach Android's telephony takes (§A.2
  there). This note pins the source and the code path.
- TBB's zones extend to the coverage halo, so the main Nepal pack holds 6 areas, including
  India's and China's. The lite pack covers only its own country's outline. A point in the
  halo outside the country has no zone in lite. That is honest: lite has no clean source for
  the neighbours' borders until their own country packs exist.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| TBB | Exact borders, every zone | Drawn from OSM | ODbL: refused in lite | 2026f | main only |
| tzdb `zone.tab` + admin-0 outline | Exact for one-zone countries; no new geometry; same IDs as TBB | Several-zone countries need a curated table | Public domain + the admin source's licence | 2026f | **chosen** |
| Natural Earth time zones (10m) | One global file | Offset polygons rather than IANA IDs, generalised, updated rarely (**unverified**: not re-fetched for this note) | Public domain | 5.x | rejected |
| GeoNames per-place `timezone` | Per place, CC BY | Points, not areas: no `tz_at` for an arbitrary coordinate | CC BY 4.0 | daily dump | use as the independent check |

## 5. What we build ourselves, and why

- **`read/tzdb`:** read `zone.tab` from the pinned tarball, with a minimal ustar reader over
  `flate2`. That is about 40 lines and no new dependency.
- **The `tzdb` zones:** the country's single zone, with the admin-0 rings of the boundary
  source as its area. The area is encoded as REGION exactly as TBB's is, and `PLACE_TZ` follows
  from it (D-134).

Gates:

- Every place inside the outline has the zone.
- `tz_of` and `tz_at` agree (the existing `verify_place_tz`).
- A country with several rows and no table is refused.

## 6. Impact on the plan

- D-138.
- Recipe `timezones` becomes a list.
- `TZ_FIRST` (the zone that wins an overlap in TBB, `Asia/Kathmandu`) moves out of code into
  the recipe as `timezone_first`, because no country may live in code (P16).
- Phase 3.7, "the lite build from clean-room sources only": timezones done; postal (Q-009) and
  population (ADR-0030) remain.

## 7. Risks and open items

- Multi-zone countries (USA, Canada, Mexico, Brazil, Russia, Australia, Indonesia and others):
  their tables come with their rollout. Admin lines are not always zone lines (parts of the
  US and Canada), so each table must state where it is approximate. Measuring it against TBB
  is a licence question (Q-012).
- Checking the GeoNames `timezone` column: a follow-up item.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| tzdb | 2026f | 2026-11-09 | data.iana.org/time-zones/releases |
| TBB | 2026f | 2026-11-06 | `data/sources.lock` |

## 9. Sources

- [IANA Time Zone Database](https://www.iana.org/time-zones), tzdata2026f.tar.gz (`LICENSE`,
  `zone.tab`, `zone1970.tab`), accessed 2026-11-09.
- [timezone-and-elevation](timezone-and-elevation.md), this repository, §6 and §A.3.
- [CLEAN_ROOM.md](../../../CLEAN_ROOM.md) and
  [ADR-0030](../../03-decisions/adr/ADR-0030-source-admission.md).
