---
title: "India data sources"
status: research
updated: 2026-11-13
owner: teigeodata maintainers
revisit: 2027-05-11
---

# India data sources

## 1. Question

Which sources can teigeodata use for India (TG-15, [Tithipatra addendum 1](../../01-product/consumers/tithipatra.md#addendum-1-offline-first-2026-11-11))?

- **Now:** towns (statutory towns and census towns, about 8,000) with names in English and
  Devanagari (Hindi), heights, the time zone (`Asia/Kolkata`), and states and districts with
  ISO 3166-2 codes. These go into an embedded layer and an India pack.
- **Later:** villages (about 650,000) as their own downloadable component.

For each source this note records the licence, whether it may enter **main** (ODbL, may read
OSM) and **lite** (CC BY 4.0, clean-room, no ODbL or share-alike input at any step), and how
it sits with the Nepal perspective ([ADR-0006](../../03-decisions/adr/ADR-0006-perspective.md)).
It feeds the India source manifest, `data/countries/in/` (profile, `iso3166.yaml`,
golden queries), the provenance gate's allow-list, and the embedded-layer size plan
([embedded layer](embedded-layer.md)).

The general licence rules come from the [source licence matrix](source-licences.md) §3.12,
which this note extends rather than repeats. This is a research note, not legal advice.

## 2. Summary and recommendation

- **The spine is official lists, joined by code.** The town list is the Census 2011 town
  directory (7,933 towns final, 7,935 provisional: 4,041 statutory towns and 3,892–3,894
  census towns) joined to the **Local Government Directory (LGD)**. On 2026-11-11, LGD had
  36 states and union territories, 790 districts, 7,112 sub-districts, 677,660 villages and
  5,050 urban local bodies. Of those bodies, 3,858 carry a Census 2011 town code. LGD gives
  the hierarchy (town → sub-district → district → state) as attribute codes, **with no
  polygon needed**. That makes the lite admin chain possible even without open district
  outlines.
- **Licences of the spine.** The Census and LGD copies on data.gov.in are under GODL. That is
  attribution plus non-endorsement, with no share-alike, so it suits both builds (verified
  2026-11-06 in the matrix). LGD's own site now has a **copyright policy** (read 2026-11-11):
  material "may be reproduced free of charge in any format or media without requiring
  specific permission", if it is reproduced accurately, not used in a misleading context,
  and the source is acknowledged. That closes the matrix's open item "LGD terms
  unverified" for both builds. Census pages on censusindia.gov.in say "All Rights Reserved",
  so use only the data.gov.in (GODL) copies.
- **Points have no official open source.** Neither the Census nor LGD publishes coordinates.
  - **Lite** takes points from GeoNames (CC BY 4.0) and Wikidata (CC0). The post-office
    coordinates in the All India Pincode Directory (GODL) are a last resort.
  - **Main** adds OSM `place=*` nodes (ODbL).
  - A first measurement: an exact name match within the state finds a GeoNames point for
    3,923 of 5,050 urban local bodies (77.7%). It resolves 3,159 of them to a single
    populated GeoNames place (62.6%). The rest need the district constraint and Wikidata
    (`P6425` LGD local body code, `P5578` census code).
  - SHRUG polygons are CC BY-NC-SA, so neither build may use them.
- **Hindi names are the gap.**
  - Wikidata `hi` labels cover most cities and towns that have an item: 508 of 567 cities,
    774 of 826 districts, 644 of 1,114 census towns.
  - GeoNames has `hi` names for only 731 of its 6,596 Indian places of 5,000 or more people.
  - LGD's "Name (In Local)" column is in each **state's** official language, not Hindi. Its
    Devanagari is filled for 1,218 of 5,050 urban bodies and 22% of villages, but only 22
    of Uttar Pradesh's 770 urban bodies. Maharashtra's Devanagari is **Marathi** (`mr`),
    not Hindi.
  - The rest needs the TG-14 transliteration path, labelled as such.
- **Boundaries under the Nepal view.**
  - India's units are cut by Nepal's claim polygon: Kalapani, Lipulekh and Limpiyadhura go
    to Nepal. Main uses OSM relation 13559363 (D-054); lite uses the clean-room claim
    polygon (D-206). Places inside the claim (Gunji, Kuti, Garbyang, Nabhidang, …) belong to
    the Nepal pack and are dropped from the India pack.
  - Natural Earth's own Nepal view does **not** do this. Its line gives Kuti to India
    (measured below), so it must not decide this border.
  - Kashmir follows D-150/D-156. In the Nepal view, Gilgit-Baltistan and Azad Kashmir hang
    under India with their own codes (`PK-GB`, `PK-JK`), while Aksai Chin and the Shaksgam
    valley are China's.
- **Outlines.**
  - **Main:** states (OSM `admin_level=4`) and districts (`admin_level=5`) come from
    Overture divisions or OSM.
  - **Lite:** states come from Natural Earth admin 1 (public domain, already in the
    baseline). Districts are **code-only units** until an open district polygon is
    confirmed. DataMeet's 2011 districts are CC BY 2.5 IN but stale (2011) and of unclear
    lineage. The Survey of India's free boundary download has terms we could not read.
- **ISO 3166-2:IN** has 36 codes (28 states, 8 union territories) after the 2023-11-23
  changes (`IN-OR`→`IN-OD`, `IN-CT`→`IN-CG`, `IN-TG`→`IN-TS`, `IN-UT`→`IN-UK`). This is
  consistent with [ISO codes](iso-3166-codes.md).
- **Heights** use the Nepal method unchanged: Copernicus GLO-30 in main, NASADEM in lite.
  Both cover all of India.
- **Size** (estimate from the measured per-place costs in [sizes](../../05-data/sizes.md),
  2026-11-11):
  - 8,000 towns are about 250 kB on the wire with names, heights, admin and search, or
    about 410 kB with SUBSTRING, before outlines.
  - Villages are about 20 MB without SUBSTRING.

## 3. Findings

### 3.1 Town lists

**Census of India 2011.**

- Provisional counts: 7,935 towns, of which 4,041 statutory and 3,894 census towns ([MoHUA
  table](https://mohua.gov.in/cms/number-of-cities--towns-by-city-size-class.php)). Later
  summaries give 7,933 and 3,892. Which one the final town directory holds is
  **unverified** until we count its rows.
- Code scheme (MDDS, PLCN): state 2 digits, district 3, sub-district 5, village/town 6.
  Villages use 000001–799999 and statutory towns 800001–899999. Census towns keep a code in
  the village range. Codes of merged or split units are frozen. Source: the ORGI catalogue
  entry `PC11_MDDS_R-10`.
- Rights: catalogue pages on censusindia.gov.in say "© ORGI Digital Library, All Rights
  Reserved", with no licence.
- data.gov.in carries "Complete Town Directory by India/State/District/Sub-District Level,
  Census 2011" per state. The portal footer says content is "licensed under the Government
  Open Data License-India". The resource pages render only with JavaScript, so the
  per-resource licence field and the columns are **unverified**. Use the data.gov.in copies
  only.
- The town directory has **no coordinates** (from its field lists; **unverified** on a full
  file).
- Census 2027: the house-listing phase ran 2026-04-01 to 2026-09-30 in state windows. The
  population count's reference date is 2027-03-01 (2026-10-01 for snowbound areas). No new
  town list exists yet; the 2027 town list will follow the count. Revisit at this note's
  revisit date.

**Local Government Directory (LGD, Ministry of Panchayati Raj, built by NIC).**

- Live counts from the home page, 2026-11-11: 36 states and UTs, 790 districts, 7,112
  sub-districts, 7,328 blocks, 677,660 villages (658,151 inhabited, 18,371 uninhabited,
  1,138 forest), 5,050 urban local bodies (276 municipal corporations, 2,006 municipalities,
  2,416 town panchayats, the rest other types).
- Fields, from the source headers of the daily dump (below):
  - States: code, version, name in English, name in local language, Census 2001 and 2011
    codes, state or UT.
  - Villages: also category (Rural, Urban, Both, Unmapped) and status (Inhabitant,
    Un-Inhabitant, Forest).
  - Urban local bodies: code, version, English and local names, type code, Census 2011 code.
- Census link: 3,858 of the 5,050 urban local bodies carry a Census 2011 town code. Some
  2011 codes are `0` for units created after 2011 (Telangana, Ladakh, the merged Dadra and
  Nagar Haveli and Daman and Diu). LGD and ORGI are building a "Uniform Jurisdictional
  Directory" that ties LGD codes to the census frame (home page text).
- **Terms, read on 2026-11-11** (`copyRightPolicy.do`, `termsconditions.do`):
  - Copyright policy: material "may be reproduced free of charge in any format or media
    without requiring specific permission". The conditions are that it is reproduced
    accurately, not used "in a derogatory manner or in a misleading context", and the
    source is "prominently acknowledged". Third-party material is excluded.
  - Terms and conditions: no liability, Indian law and courts.
  - This is the standard Government of India website clause, not a database licence. It
    places no restriction on commercial use and no share-alike. "Reproduced accurately" fits
    codes and official names. Our own normalised spellings must be labelled as ours.
  - data.gov.in also lists "Local Government Directory (LGD) - Villages" and "- Local
    Bodies" under the portal's GODL footer (per-resource field **unverified**).
- **Access:**
  - The download page has a captcha. The NAPIX API needs registration.
  - A third-party daily mirror converts every LGD report to Parquet with SHA-256 per file and
    a `metadata.json` index ([ramSeraph/opendata](https://ramseraph.github.io/opendata/lgd/)).
    It was observed 2026-11-11; the villages file is 7.99 MB for 677,660 rows. The repo has
    no licence of its own ("NOASSERTION"), so use it only as a transport, credit LGD, and
    keep LGD as the source of record. An older mirror
    ([planemad](https://github.com/planemad/india-local-government-directory), 2022-03-11)
    states GODL.

**GeoNames IN** (CC BY 4.0). Dump modified 2026-11-11, 15.8 MB zipped, 660,064 records.
Measured on that dump:

| Class | Count | With population | ≥ 5,000 | ≥ 15,000 |
|---|--:|--:|--:|--:|
| PPL | 546,172 | 7,073 | 6,306 | 3,565 |
| PPLA, PPLA2–5, PPLC | 211 | 180 | 176 | 161 |
| PPLX, PPLL, PPLQ, other P | 11,650 | 129 | 114 | 62 |
| All populated places (P) | 558,033 | 7,382 | 6,596 | 3,788 |

- Coverage below 15,000 is real: 2,808 places of 5,000–15,000 people carry a population.
- Most of the 546k PPL have no population and many share a name: Uttar Pradesh alone has
  dozens of "Kuti" and "Patna". Unweighted name matching is ambiguous.
- Admin 3 (6,891 `ADM3`) mirrors the sub-districts.
- Time zones: 660,011 records `Asia/Kolkata`. The 53 others are border spill, including 23
  `Asia/Kathmandu`.
- GeoNames files Kuti, Gunji and Garbyang under Uttarakhand (`IN.39`) with `Asia/Kolkata`.
  Under the Nepal view they belong to Nepal (§3.3).

**Wikidata** (CC0). Counts from `query-main.wikidata.org` on 2026-11-11. The default
endpoint was rate-limited at 1 request per minute during an outage.

| Selection | Items | With coordinates | With `hi` label |
|---|--:|--:|--:|
| `P31` census town of India (Q16830604) | 1,114 | 834 | 644 |
| `P31` town in India (Q58339518) | 218 | 217 | 191 |
| `P31` city (Q515), country India | 567 | 550 | 508 |
| `P31` district of India (Q1149652) | 826 | 796 | 774 |
| `P6425` LGD local body code | 7,270 | 1,128 | 45 |
| `P5578` Census 2011 code, town range 8xxxxx | 372 | 362 | 336 |
| `P12745` LGD village code | 78,702 | 56,945 | not counted |
| `P31` village in India (Q56436498) | 154,325 | not counted | not counted |

- Wikidata has the properties to join on: `P5578` (Census 2011), `P3213` (2001), `P12747`
  state, `P12746` district, `P12748` sub-district, `P12745` village, `P6425` local body.
- Coverage of the town range is thin: only 372 items carry a town code.
- A wider count (population ≥ 5,000, coordinates, India) timed out. **Unverified.**

**OpenStreetMap** (ODbL, main only).

- Geofabrik `india-latest.osm.pbf` is 1.6 GB, with data to 2026-11-09T21:22Z.
- The India `admin_level` scheme on the OSM wiki: 4 state, 5 district, 6 sub-district
  (tehsil, mandal, taluk), 8 municipal body, 10 village or civic ward.
- The counts of `place=city/town/village` could not be read: Overpass returned errors.
  **Unverified.** Measure them from the extract at the first build.

### 3.2 Points and coordinates

| Source | Gives | Licence | Main | Lite | Note |
|---|---|---|---|---|---|
| GeoNames IN | points for about 7.4k populated places, plus 550k unpopulated name points | CC BY 4.0 | ✓ | ✓ | ambiguous names; positions mostly from older gazetteers |
| Wikidata | points for about 2.3k town and city items; 57k village items via `P12745` | CC0 | ✓ | ✓ | drop OSM-imported statements in lite (matrix §3.4) |
| All India Pincode Directory | post-office lat/lon | GODL (portal footer) | ✓ | ✓ | accuracy unknown, some values swapped or blank (**unverified**); last resort |
| OSM `place=*` | the densest current points | ODbL | ✓ | ✗ | the main build's first choice |
| SHRUG 2.2 (Aug 2026), "Census village polygons" on AIKosh | 2011 village and town polygons | CC BY-NC-SA 4.0 | ✗ | ✗ | non-commercial, share-alike |
| DataMeet `indian_village_boundaries` | village polygons, some states | ODbL | ✓ | ✗ | community digitised, last push 2018 |
| Bhuvan (ISRO) | village and town layers | terms not open (**unverified**) | ✗ | ✗ | |
| Survey of India online maps portal | "administrative boundary database", free on request | terms not read (**unverified**) | ✗ | ✗ | the page failed TLS verification |

**Match measurement** (2026-11-11). We joined LGD's 5,050 urban local bodies to GeoNames IN
populated places on lower-cased name (suffixes such as "Municipal Corporation" stripped) and
state:

- 3,923 bodies matched at least one place (77.7%).
- 2,520 matched exactly one place.
- 3,159 matched exactly one place with a population (62.6%).
- 266 matched only ambiguous places with no population.

Census towns are not urban local bodies, so their rate must be measured on the town
directory. The rest calls for the district (GeoNames `admin2`, or a district polygon in
main), Wikidata codes, and in main OSM. §5 sets the gate.

### 3.3 Boundaries and the Nepal perspective

**Measured on Natural Earth 5.1.2 Nepal-view admin 0** (`ne-admin0-nep`, sha256 prefix
`b56e5675c080fca9`, as pinned). Ray-cast point-in-polygon on 12 probes:

| Probe | Country in the Nepal view |
|---|---|
| Kalapani (30.20 N, 80.95 E), Gunji | NPL |
| Kuti (30.296 N, 80.762 E) | **IND** |
| Susta (27.53 N, 83.80 E) | NPL |
| Gilgit, Muzaffarabad, Siachen, Srinagar, Leh | IND |
| Aksai Chin (35.2 N, 79.3 E), Shaksgam (36.0 N, 76.6 E) | CHN |
| Tawang (Arunachal Pradesh) | IND |

- Natural Earth's Nepal view draws the old Kalapani claim, **not** the 2020 map's
  Limpiyadhura extent. Kuti lies in Nepal's 2020 claim (D-054 places it inside OSM relation
  13559363).
- So the India layer must be cut by the project's own claim polygon:
  - main: OSM relation 13559363, joined as for Nepal's admin 0 (D-054);
  - lite: the clean-room polygon digitised from the Survey Department's 2020 map (D-206).
    Until that polygon exists, lite flags the area as disputed with no unit, exactly as for
    Nepal.
- Cutting Pithoragarh district (Dharchula sub-district) and Uttarakhand by the claim leaves
  India's outline outside it. Places inside the claim go to the Nepal pack with Nepal units
  and `Asia/Kathmandu`, and the India pack drops them by containment, not by source
  country (D-147 rule).
- Susta (Nepal view: NPL) needs the same treatment if Nepal's official map shows it.
  **Unverified**: check the 2020 map and OSM for a disputed relation.

**Kashmir and other disputes** follow D-150 and D-156. Ownership of disputed units comes from
the Nepal view, while India's own units keep OSM's (main) or Natural Earth's (lite) lines:

- Gilgit-Baltistan and Azad Kashmir are owned by India in this view. They stay units with
  their own ISO codes, `PK-GB` and `PK-JK`, under India
  ([ISO codes](iso-3166-codes.md)).
- Aksai Chin and the Shaksgam valley are China's. India's `IN-LA` and `IN-JK` outlines
  therefore exclude them.
- Arunachal Pradesh is India's.
- The India pack's extent is Nepal-view India. Places in Gilgit-Baltistan and Azad Kashmir
  come from the Pakistan sources, the world part, or a later Pakistan pack. The India
  sources (LGD, Census) do not list them. They are not in the India towns component.

**Outline sources.**

| Source | Levels | Licence | Main | Lite | Note |
|---|---|---|---|---|---|
| Overture divisions / OSM | states (4), districts (5), sub-districts (6) | ODbL | ✓ | ✗ | current with LGD's 790 districts (**unverified** count) |
| geoBoundaries IND | ADM1 CC BY 2.5 IN; ADM2–5 ODbL | mixed | ✓ | ADM1 only | ADM1 is 2011 (matrix §3.6) |
| Natural Earth admin 1 5.1.2 | states | public domain | ✓ | ✓ | coarse (1:10m); already in the baseline (D-150) |
| DataMeet `maps` Districts | 2011 districts | CC BY 2.5 IN | ✓ | ◐ | built from GeoCommons and Bhuvan taluks and the Census atlas: lineage unclear; 2011, not today's 790 |
| DataMeet `Country/india-composite` | India's claimed outline | CC0 (README) | ✓ | ✓ | India's view: useful later for an `in` perspective |
| Survey of India | state and district database | **unverified** | ✗ | ✗ | the official India view |

**India's law.** India's Geospatial Guidelines of 2021 make Survey of India maps or boundary
data the standard for political maps of India. The source is law-firm summaries and the OSM
wiki; the DST primary text was not read, so this is **unverified**. Our Nepal-view default
differs from it in Aksai Chin, Kalapani, Lipulekh and Limpiyadhura. ADR-0006 already says
docs warn app developers; an `in` perspective is possible as data later.

### 3.4 Hindi names

| Source | Coverage measured 2026-11-11 | Language truth | Licence |
|---|---|---|---|
| Wikidata `hi` labels | cities 508/567, districts 774/826, census towns 644/1,114, towns 191/218 | hand- and bot-made Hindi | CC0, both builds |
| GeoNames `hi` alternate names | 1,381 features; 731 of 6,596 places of 5,000 or more people | tagged `hi` | CC BY 4.0, both builds |
| LGD "Name (In Local)" | Devanagari in 1,218/5,050 urban bodies and 150,900/677,660 villages (22.3%). By state: Maharashtra 99.9%, Himachal 99.9%, Chhattisgarh 97.3%, Bihar 51.3%, Jharkhand 31.1%, MP 18.1%, Rajasthan 15.2%, UP 9.3% | the **state's** official language: Marathi in Maharashtra, Telugu in Telangana, etc.; often just the English name in capitals | LGD policy / GODL, both builds |
| Census 2011 Hindi tables | the location code directory page has a Hindi toggle; whether the files hold Hindi names is **unverified** | official | censusindia "All Rights Reserved"; only GODL copies |
| OSM `name:hi` | not counted | community | ODbL, main only |

**Rule for `hi`:**

- LGD's local name becomes `hi` only where the state's official language is Hindi (UP,
  Bihar, MP, Rajasthan, Chhattisgarh, Jharkhand, Haryana, Himachal, Uttarakhand, Delhi) and
  the text is in Devanagari.
- In Maharashtra it is `mr`, in Goa `kok` or `mr`, and so on. The language comes from a
  per-state table in `data/countries/in/profile.yaml`, not from the script.
- Fallback chain (as in the embedded-layer plan): official `hi`, then Wikidata `hi`, then
  GeoNames `hi`, then OSM `name:hi` (main), then the TG-14 transliteration, flagged as
  machine-made.

### 3.5 Heights

Same method as Nepal ([elevation method](../../03-decisions/adr/ADR-0034-elevation-method.md)):

- main: Copernicus GLO-30 (DGED COG on AWS);
- lite: NASADEM (60° N–56° S).

Both cover all of India: the mainland lies between 6° N and 37° N, and the islands are
inside both grids. Copernicus GLO-30 withholds no Indian tiles (from the product's
documentation, not re-read today).

Only the tiles that hold a town are needed: about 350 1° tiles for towns, and more for
villages, read one at a time with the disk budget. The only new risk is size on disk while
streaming.

### 3.6 Time zone

All of India is `Asia/Kolkata` (tzdb `zone1970.tab`; GeoNames 660,011 of 660,064). Under the
Nepal view, places in the Kalapani claim get `Asia/Kathmandu` from the claim polygon, as for
Nepal (D-054). Lite zones come from admin polygons ([lite time zones](lite-timezones.md)).
For India that is a single country polygon, with no sub-national split.

### 3.7 ISO 3166-2:IN

There are 36 codes: 28 states and 8 union territories. Source: Wikipedia's ISO 3166-2:IN,
citing ISO OBP, read 2026-11-11, and consistent with [ISO codes](iso-3166-codes.md) §3.

States:

| Code | Name | Code | Name |
|---|---|---|---|
| `IN-AP` | Andhra Pradesh | `IN-MN` | Manipur |
| `IN-AR` | Arunachal Pradesh | `IN-ML` | Meghalaya |
| `IN-AS` | Assam | `IN-MZ` | Mizoram |
| `IN-BR` | Bihar | `IN-NL` | Nagaland |
| `IN-CG` | Chhattisgarh | `IN-OD` | Odisha |
| `IN-GA` | Goa | `IN-PB` | Punjab |
| `IN-GJ` | Gujarat | `IN-RJ` | Rajasthan |
| `IN-HR` | Haryana | `IN-SK` | Sikkim |
| `IN-HP` | Himachal Pradesh | `IN-TN` | Tamil Nadu |
| `IN-JH` | Jharkhand | `IN-TS` | Telangana |
| `IN-KA` | Karnataka | `IN-TR` | Tripura |
| `IN-KL` | Kerala | `IN-UP` | Uttar Pradesh |
| `IN-MP` | Madhya Pradesh | `IN-UK` | Uttarakhand |
| `IN-MH` | Maharashtra | `IN-WB` | West Bengal |

Union territories: `IN-AN`, `IN-CH`, `IN-DH`, `IN-DL`, `IN-JK`, `IN-LA`, `IN-LD`, `IN-PY`.

Changes since 2014:

| Date | Change |
|---|---|
| 2014-10-30 | `IN-TG` added; Orissa renamed Odisha (code still `IN-OR`) |
| 2019-11-22 | `IN-JK` becomes a union territory; `IN-LA` added |
| 2020-11-11 | `IN-DH` replaces `IN-DN` and `IN-DD` |
| 2023-11-23 | `IN-OR`→`IN-OD`, `IN-CT`→`IN-CG`, `IN-TG`→`IN-TS`, `IN-UT`→`IN-UK` |

The ISO OBP page itself renders only with JavaScript, so the source is the citation
(**partly verified**).

Join to LGD by state code: LGD has 36 states and UTs, which matches. LGD's state codes
(Census-style, e.g. Telangana 36, Ladakh 37, DNHDD 38) are not ISO codes. The mapping lives
in `data/countries/in/iso3166.yaml`. ISO has no district codes; districts carry LGD codes.

### 3.8 The town join, measured (2026-11-13)

Measured on the LGD mirror of 2026-11-12 (`urban_local_bodies` 5,050 rows,
`statewise_ulbs_coverage` 5,098 bodies, `villages` 677,660 rows, `districts` 790), GeoNames
`IN.zip` of 2026-11-12 and QLever's Wikidata. Scripts were throwaway; the numbers go into the
forge's own report when the stage is built.

- **Census towns are not in LGD.** A census town is a village-range code the Census calls
  urban. LGD files 22,486 inhabited villages as `Urban`, and all but 74 of them lie inside an
  urban local body (the villages a statutory town covers). So LGD gives the **statutory towns
  only**; census towns need the Census 2011 town directory, whose data.gov.in API takes a key
  (Q-024).
- **Each urban body's district is known.** `statewise_ulbs_coverage` maps every body to its
  district(s), sub-district(s) and villages (27 bodies span two districts). LGD's urban body
  file itself has only the state.
- **Districts link to GeoNames through Wikidata.** Items with an LGD district code (`P12746`)
  carry a GeoNames ID (`P1566`) for 662 of 790 districts, 652 of them a GeoNames `ADM2`: an
  exact crosswalk. 334 body-district links more match by state and district name; 330 have
  neither (districts made after GeoNames' list).
- **Wikidata does not link the bodies.** Only 101 of the 5,050 bodies have an item with
  `P6425`, 7 with a GeoNames ID: `P6425` is mostly on panchayats. No help for points.
- **Points by name within the district** (GeoNames populated places, type words such as
  "Municipal Council" stripped):

  | Match | Exact name key | Phonetic key |
  |---|--:|--:|
  | one place in the body's district | 3,592 (71.1%) | 3,946 (78.1%) |
  | one place in the state, no district link | 230 (4.6%) | 203 (4.0%) |
  | ambiguous, or only outside the district | 260 (5.1%) | 497 (9.8%) |
  | no name match | 968 (19.2%) | 404 (8.0%) |

  The phonetic key (aspirates folded, vowels after the first letter dropped, doubles
  collapsed) finds Mahabubnagar (Mahbubnagar) and Tadipatri (Tādpatri). The misses left are
  compound names ("Chamoli Gopeshwar", "S.A.S.Nagar - Mohali", "Sirhind Fatehgarh Sahib"),
  which match on one of their parts, and bodies whose GeoNames place is missing. Ambiguity is
  mostly a town and its own suburb or bazaar of the same name in one district, broken by
  population and the seat class (`PPLA*`).
- **So the research's ≥ 95% gate is out of reach in lite with these sources**: about 82% of
  statutory towns take a point without a guess, perhaps 88–90% with compound names and the
  population tie-break. Lite reports the rest and leaves them out (no point is guessed, §5);
  main adds OSM `place=*` and keeps the 95% target.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Census 2011 town directory (data.gov.in) | the definition of "town"; codes; civic status; population | 2011; no points; the census-town list changes in 2027 | GODL (footer) | 2011, final | **spine, both builds** |
| LGD | current; daily; codes to village; Census 2011 crosswalk; local names | no points; local name ≠ Hindi; captcha / API key | LGD copyright policy; GODL on data.gov.in | live 2026-11-11 | **spine, both builds** |
| GeoNames IN | points; population for about 7.4k; CC BY | name ambiguity; positions of mixed origin; few `hi` | CC BY 4.0 | 2026-11-11 | **points, both builds** |
| Wikidata | `hi` labels; code properties for joins; points | sparse code coverage (372 town codes) | CC0 | live | **names and joins, both builds** |
| OSM | densest current points and outlines; `name:hi` | ODbL; India's admin data quality uneven | ODbL | 2026-11-09 | **main only** |
| Pincode directory points | official; GODL | post offices, not towns; accuracy unknown | GODL | monthly | lite last resort |
| SHRUG | town and village polygons tied to census codes | NC-SA | CC BY-NC-SA 4.0 | 2.2 (Aug 2026) | **excluded** |
| DataMeet districts | open, 2011 | stale, unclear lineage | CC BY 2.5 IN | 2022 push | hold; lite districts stay code-only |
| Natural Earth admin 1 | public domain; already gated | 1:10m | PD | 5.1.2 | **lite states** |

## 5. What we build ourselves, and why

1. **Town table.** The Census 2011 town directory is joined to LGD urban bodies and villages
   (category Urban) through the Census 2011 code. Each town carries:
   - its LGD code, where it has one (the stable source key, ADR-0008);
   - its Census 2011 code;
   - its civic status: municipal corporation, municipality, town panchayat or nagar
     panchayat, cantonment board, census town, or other;
   - its sub-district, district and state codes.

   Statutory towns created after 2011 come from LGD alone. Measurement: the count per state
   against the Census totals, and LGD bodies left unmatched.
2. **Point conflation.** For each town, candidates are taken within its district:
   - lite: GeoNames and Wikidata;
   - main: these plus OSM.

   Score by name (lexicon normalisation, ADR-0031), class and population. A town takes a
   point only when one candidate wins clearly. Otherwise it is reported, not guessed.

   Gates:
   - match rate per state (target ≥ 95% of towns with a point);
   - every point inside its own district (main: OSM polygon; lite: checked against the
     GeoNames `admin2` code);
   - a hand-checked sample of 200 within 2 km of the town's centre.

   Prove each gate by planting a town in the wrong district.
3. **Perspective cut.** India's units minus Nepal's claim polygon. Places inside it are
   dropped from the India pack. Gate: Gunji, Kuti, Garbyang and Nabhidang are absent from
   India and present in Nepal; `admin_at` at Kalapani answers Nepal. Prove it by planting a
   claim-area place in India.
4. **Hindi names.** The per-state language table and the fallback chain in §3.4. A report
   counts towns per name origin (official, Wikidata, GeoNames, OSM, transliterated).
   Target: 100% with a `hi` name, and the share transliterated reported.

**Golden gate.** `data/countries/in/golden-curated.yaml` follows the Nepal file's schema. The
expected IDs below are GeoNames IDs from the 2026-11-11 dump. IDs marked "build" are looked
up at the first build.

| Class | Queries (expected) |
|---|---|
| exact Latin, large | Varanasi (1253405), Lucknow (1264733), Patna (1260086), Kanpur (1267995), Mumbai (1275339), Kolkata (1275004), Chennai (1264527), Bengaluru (1277333), Hyderabad (1269843), Jaipur (1269515) |
| Devanagari (Hindi) | वाराणसी (1253405), लखनऊ (1264733), पटना (1260086), प्रयागराज (1278994), भोपाल (1275841), देहरादून (1273313), रांची and राँची (1258526, variant spelling) |
| historical or renamed | Benares and Banaras (1253405), Kashi (1253405, top 3), Allahabad (1278994), Bombay (1275339), Madras (1264527), Calcutta (1275004), Gurgaon (1270642), Baroda (1253573), Poona (1259229), Trivandrum (1254163), Port Blair and Sri Vijaya Puram (1259385, the 2024 rename) |
| homonyms across states (top 1 is the larger, the other in top 5) | Aurangabad (Maharashtra 1278149, Bihar build), Bilaspur (Chhattisgarh build, Himachal 1275631), Raipur (1258980 over 1258972), Udaipur (1253986 over Tripura 1253987), Gorakhpur (1270927 over Haryana 1270926) |
| small towns (5k–50k) | Raxaul (1258294), Jogbani (1268855), Nautanwa (1261672), Banbasa (7117985), Kavaratti (1267390), Kargil (1267776), Leh (1264976) |
| census towns | Kirari Suleman Nagar (build), Sultanpur Majra (build), Karawal Nagar (build) |
| prefix (3 code points) | "Var" → Varanasi in top 5; "वारा" → Varanasi in top 5 |
| perspective | "Gunji" and "Kuti" → their Kalapani-area entries with country NP and `Asia/Kathmandu`, never IN; `admin_at(30.20, 80.95)` → Nepal; `admin_at(35.2, 79.3)` → China; `admin_at(35.92, 74.31)` → India, unit `PK-GB` |
| codes | `iso_code` of Varanasi's state = `IN-UP`; Bhubaneswar → `IN-OD`; Hyderabad → `IN-TS`; Dehradun → `IN-UK`; Raipur → `IN-CG` (catches stale pre-2023 codes) |

Scoring:

- Same metrics as the search gate ([quality bar](../../06-engineering/quality-bar.md)):
  recall@1, recall@5 and MRR per class, each entry with its own `max_rank`.
- The perspective and code rows are exact assertions (pass or fail, no score).
- Bars to start with:
  - exact Latin and Devanagari: recall@1 = 100%;
  - historical: recall@3 ≥ 90%;
  - homonyms: the larger one at rank 1, the other in the top 5;
  - small and census towns: recall@5 = 100%.
- Each class gets a planted failure: a dropped `hi` name, a wrong ISO code, a claim-area
  place left in India.

That is 50 entries in all.

## 6. Impact on the plan

- **Source manifest, `data/sources.lock`:**
  - LGD (via data.gov.in, or the mirror pinned by SHA-256 with LGD as source of record);
  - the Census 2011 town directory (data.gov.in);
  - GeoNames IN, the alternate names for IN, and Wikidata;
  - OSM India (main only);
  - NASADEM and GLO-30 tiles for India.
- **Licence matrix (source-licences.md §3.12):** LGD terms now found (copyright policy,
  2026-11-11). The "Districts and below blocked" row should change to "district *codes*
  allowed in lite via LGD; district *polygons* still blocked". Proposed, for the maintainer
  or the next licence pass; not edited here.
- **`data/countries/in/`:**
  - `profile.yaml` gains the per-state official-language table for local names, and LGD
    unit types;
  - `iso3166.yaml` gets the 36 codes and the 2023 `renamed` rows;
  - `golden-curated.yaml` gets the §5 set;
  - the lexicon's `proposed` terms are used in name normalisation.
- **Embedded layer:** India's towns as points, names (en, hi), admin codes and heights.
  District outlines are optional at a TG-16 level of detail. The estimate is in §7.
- **New decisions to record:**
  - India towns spine = Census 2011 + LGD;
  - lite districts are code-only;
  - the India pack is cut by Nepal's claim polygon, not Natural Earth's Nepal view;
  - the per-state local-language rule.
- **Open question to add:** confirm the Susta dispute's treatment against Nepal's 2020 map.

## 7. Risks and open items

**Size estimate.** Per-place wire costs measured for Nepal on 2026-11-11 ([sizes](../../05-data/sizes.md)):
point 5.2 B, elevation 1.3 B, admin 1.2 B, names (2 languages) 9.2 B, search 14–16 B,
substring 19–20 B.

| Component | 8,000 towns | 650,000 villages |
|---|--:|--:|
| point | 42 kB | 3.4 MB |
| elevation | 10 kB | 0.8 MB |
| admin | 10 kB | 0.8 MB |
| names (en, hi) | 80 kB | 6.5 MB |
| search | 120 kB | 9.8 MB |
| **subtotal** | **about 260 kB** | **about 21 MB** |
| substring (derived on device, D-213) | 160 kB | 13 MB |
| 36 states + 790 districts as units | about 25 kB | – |

- Outlines are extra and not measured. State and district outlines at a 100 m level of
  detail are probably 0.3–1 MB (**unverified**, measure with `pack-sizes`).
- Indian names are longer than Nepali ones, and dual-script names cost more. Treat the
  names row as ±30%.
- The towns part sits well inside the embedded plan's "India's towns under 1 MB".

**Risks.**

- **Legal, India.** The Nepal-view outline of India differs from the Survey of India's
  outline (Aksai Chin, Kalapani area, PoK in the other direction). App developers in India
  may need the `in` perspective. The docs say so (ADR-0006).
- **GODL.** A provider may revoke a dataset (§7(c)), and arbitration is under Indian law.
  Packs need a takedown path. The non-endorsement wording goes in `ATTRIBUTION.md`.
- **LGD** is a website policy, not a licence. "Reproduced accurately" means we mark our own
  normalised names as ours. Written confirmation from MoPR or NIC would remove doubt; only
  the maintainer can ask.
- **Census-town churn.** Census 2027 will reclassify towns. Pin 2011 now; make the town flag
  a dated attribute; rebuild when the 2027 town directory is published.
- **Point quality.** GeoNames' Indian positions are of mixed origin. Some ambiguity remains
  after the district filter (266 urban bodies have only unpopulated homonyms). The gates in
  §5 decide; no point is guessed.
- **Hindi coverage.** Official Hindi exists for few towns outside Bihar and Chhattisgarh.
  Transliterated names must be labelled, and the share reported (TG-14).
- **Mirror dependence.** The LGD mirror has no licence of its own and could vanish. Pin by
  hash; keep a path to LGD itself (NAPIX key, a maintainer account).

**Could not verify (2026-11-11).**

- Which count the final 2011 town directory holds (7,933 or 7,935).
- data.gov.in per-resource licence fields and columns: the pages render only with
  JavaScript, and the API needs a key.
- Whether the Census 2011 location directory files carry Hindi names.
- The Survey of India online maps portal terms (TLS certificate failed verification) and
  NGP 2022's primary text.
- OSM `place=*` counts for India (Overpass errors).
- Wikidata's wider count of Indian places of 5,000 or more people (timeout).
- The accuracy of the pincode directory coordinates.
- Susta's status in Nepal's 2020 map.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| LGD (live counts) | 36 / 790 / 7,112 / 677,660 / 5,050 | 2026-11-11 | lgdirectory.gov.in home page |
| LGD copyright policy, terms | as served | 2026-11-11 | `copyRightPolicy.do`, `termsconditions.do` |
| LGD daily mirror | observed 2026-11-12 (urban bodies 93,618 B, sha256 `68fb622b…`; coverage 489,857 B) | 2026-11-13 | GitHub release `lgd-parquet-latest` of ramSeraph/opendata |
| LGD daily mirror (first look) | observed 2026-11-11 (villages 7,991,034 B, sha256 `397ca8e9…`) | 2026-11-11 | ramseraph.github.io/opendata/lgd `metadata.json` |
| GeoNames IN dump | Last-Modified 2026-11-11 02:43 GMT, 15,755,091 B | 2026-11-11 | download.geonames.org |
| GeoNames alternate names IN | 1,371,333 B | 2026-11-11 | download.geonames.org |
| Wikidata | live counts | 2026-11-11 | query-main.wikidata.org |
| OSM India extract | data to 2026-11-09T21:22Z, 1.6 GB | 2026-11-11 | download.geofabrik.de |
| Natural Earth admin 0, Nepal view | 5.1.2 (sha256 `b56e5675…`) | 2026-11-11 | pinned `ne-admin0-nep` |
| SHRUG | 2.2 (August 2026), CC BY-NC-SA 4.0 | 2026-11-11 | devdatalab.org |
| DataMeet maps | last push 2022-05-11 | 2026-11-11 | GitHub API |
| DataMeet village boundaries | ODbL, last push 2018-03-05 | 2026-11-11 | GitHub API |
| ISO 3166-2:IN | last change 2023-11-23 | 2026-11-11 | Wikipedia (cites ISO OBP) |
| Census 2027 | house listing 2026-04-01 to 2026-09-30; reference date 2027-03-01 | 2026-11-11 | press reports |
| Copernicus GLO-30, NASADEM | as pinned for Nepal | 2026-11-11 | `data/sources.lock` |

## 9. Sources

- [Local Government Directory](https://lgdirectory.gov.in/), Ministry of Panchayati Raj / NIC, accessed 2026-11-11 (home page counts, copyright policy, terms and conditions).
- [LGD data dumps](https://ramseraph.github.io/opendata/lgd/), ramSeraph/opendata, observed 2026-11-11.
- [india-local-government-directory](https://github.com/planemad/india-local-government-directory), planemad, retrieved 2022-03-11, accessed 2026-11-11.
- [Local Government Directory (LGD) - Villages](https://www.data.gov.in/resource/local-government-directory-lgd-villages), OGD Platform India, accessed 2026-11-11 (footer only).
- [Complete Town Directory, Census 2011 (Goa)](https://www.data.gov.in/resource/complete-town-directory-indiastatedistrictsub-district-level-census-2011-goa), OGD Platform India, accessed 2026-11-11 (footer only).
- [MDDS code (PC11_MDDS_R-10)](https://censusindia.gov.in/nada/index.php/catalog/7049), ORGI digital library, modified 2021-01-19, accessed 2026-11-11.
- [Number of cities / towns by city size class](https://mohua.gov.in/cms/number-of-cities--towns-by-city-size-class.php), MoHUA, accessed 2026-11-11.
- [Government Open Data License – India](https://www.data.gov.in/sites/default/files/Gazette_Notification_OGDL.pdf), Gazette of India, 2017-02-13 (read 2026-11-06, [matrix](source-licences.md)).
- [GeoNames IN dump](https://download.geonames.org/export/dump/IN.zip) and [alternate names IN](https://download.geonames.org/export/dump/alternatenames/IN.zip), GeoNames, 2026-11-11.
- [Wikidata Query Service](https://query-main.wikidata.org/), Wikimedia, queried 2026-11-11.
- [India extract](https://download.geofabrik.de/asia/india.html), Geofabrik, accessed 2026-11-11.
- [Key:boundary=administrative, admin_level table](https://wiki.openstreetmap.org/wiki/Tag:boundary=administrative), OSM wiki, accessed 2026-11-11.
- [SHRUG download](https://www.devdatalab.org/shrug_download/), Development Data Lab, accessed 2026-11-11; [SHRUG village polygons on AIKosh](https://aikosh.indiaai.gov.in/home/datasets/details/shrug_village_level_polygon_geometries.html), accessed 2026-11-11.
- [datameet/maps](https://github.com/datameet/maps) (README, `Country/README.md`, `Districts/README.md`) and [datameet/indian_village_boundaries](https://github.com/datameet/indian_village_boundaries), DataMeet, accessed 2026-11-11.
- [ISO 3166-2:IN](https://en.wikipedia.org/wiki/ISO_3166-2:IN), Wikipedia (citing ISO OBP), accessed 2026-11-11.
- [Natural Earth admin 0, Nepal view, v5.1.2](https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/ne_10m_admin_0_countries_nep.geojson), Natural Earth, accessed 2026-11-11.
- [Census 2027: Centre notifies first phase](https://www.business-standard.com/india-news/census-2027-centre-notifies-first-phase-house-listing-april-1-126010701331_1.html), Business Standard, 2026-01-07; [Census 2027 to be conducted digitally](https://www.thestatesman.com/business/census-2027-to-be-conducted-digitally-for-the-first-time-first-phase-to-begin-on-april-1-1503575945.html), The Statesman, accessed 2026-11-11.
- [Geospatial data guidelines 2021](https://trilegal.com/knowledge_repository/geospatial-data-guidelines-2021/), Trilegal, and [India/Government of India](https://wiki.openstreetmap.org/wiki/India/Government_of_India), OSM wiki, accessed 2026-11-11 (secondary; primary DST text not read).
- [Online Maps Portal, Survey of India](https://onlinemaps.surveyofindia.gov.in/AboutPortal.aspx), accessed 2026-11-11 (search excerpt only; TLS verification failed).
