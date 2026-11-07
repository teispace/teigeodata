---
title: "Postal codes: sources, model and the POSTAL section"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# Postal codes: sources, model and the POSTAL section

## 1. Question

Phase 3 builds the postal data (D-087). This note answers:

1. Nepal, the first country: the current official scheme or schemes, the authority, any
   machine-readable list and its licence, and how codes map to admin units and post offices
   (open question Q-001).
2. Sources for the rollout (India, South Asia, USA, the world) and which go into the main build
   (ODbL) and which into the lite build (CC BY 4.0, clean-room).
3. The scheme diversity the format must handle, and a canonical normalisation of codes kept as
   per-country data (P16).
4. The queries the SDK needs.
5. The layout of the POSTAL section (kind 11), reusing `search::fst`, with sizes measured on
   real code sets.

It feeds the POSTAL layout in [pack format](../../04-architecture/pack-format.md#sections), the
postal fields in [the schema](../../05-data/schema.md),
[ADR-0022](../../03-decisions/adr/ADR-0022-timezone-and-postal.md), Q-001 and Q-009 in
[open questions](../../07-planning/open-questions.md), and the Nepal and India source manifests.

## 2. Summary and recommendation

- **Q-001 is answered from a primary source.** The General Post Office (GPO, Department of
  Postal Services) published the federal scheme as an HTML table on its postal code page. The
  Internet Archive holds identical copies from 2025-09-08, 2026-01-09, 2026-07-20 and
  2026-10-01. Its columns are "code (local level)" and "code (local level and ward)". It lists
  **753 local levels with a 5-digit code each and 6,743 wards with a 7-digit code each**, where
  the ward code is the local-level code followed by the 2-digit ward number. Our parse of all
  753 rows found no exception. The 5-digit code is the CBS local-level code: the first digit is
  the province (checked for all 753 rows), then two digits for the district within the province
  and two for the local level (Phaktanglung 10101, Kathmandu Metropolitan City 30608, Kirtipur
  30609). Codes belong to **areas** (local level, ward), not to post offices. The 1991 scheme
  (5-digit, district plus post office, for example 44600) is a different scheme that has been
  replaced. **67 of the 5-digit strings exist in both schemes** with different meanings, so the
  scheme must be part of every key.
- **Still unverified for Nepal:** the content of the "UPU Postal Code English" PDF that replaced
  the table between 2026-10-01 and 2026-11-06 (the server refuses downloads to us); the
  legal date the scheme took effect and whether 1991 codes are still accepted; any licence. The
  GPO page states no licence.
- **Sources:** Nepal codes are derived from CBS codes plus ward numbers (facts), checked against
  the GPO table, in both builds. India uses the India Post directory (GODL) in both builds, with
  GeoNames as a fallback. The USA uses ZCTA 2026 (public domain) for areas and GeoNames for
  non-ZCTA ZIPs, in both builds. The world uses GeoNames postal (CC BY 4.0, 123 countries) in
  both builds, plus OSM `postal_code` boundaries in main only. Details in §3.6.
- **Model:** every postal record links to a PLACES row: the admin unit for admin-aligned schemes
  (Nepal), or a settlement or `postal_area` place for post offices and points. Normalisation
  rules are data per scheme, in `data/countries/<cc>/postal.yaml`, and the forge compiles them
  into the pack so the reader normalises with no country code in the library.
- **Layout:** one `search::fst` automaton whose keys are the **scheme name, a 0x00 byte, then
  the canonical code**. The key's ordinal indexes column-table record lists (place row, kind,
  optional validity and redirect), plus an optional reverse index from place to codes.
  **Estimated sizes**, from a Python model of the encodings: Nepal 2025 is 7,496 keys in
  27.8 KB; India is 19,238 PINs and 155,570 records in 575 KB; US ZCTA is 33,791 keys in
  158 KB; GB full is 1.84 M postcodes in 9.20 MB; the world GeoNames set is 4.28 M keys and
  5.01 M records in 23.7 MB, or 14.1 MB without the reverse index.
- **Point → postal code:** use the admin REGION and hierarchy for admin-aligned schemes; a postal
  REGION layer where open postal polygons exist (US ZCTA; OSM in main); otherwise the nearest
  postal place through PLACE_INDEX, flagged as approximate.

## 3. Findings

### 3.1 Nepal: the 2025 federal scheme (Q-001)

**Authority.** The Department of Postal Services (हुलाक सेवा विभाग), under the Ministry of
Communication and Information Technology. Its General Post Office (गोश्वारा हुलाक कार्यालय,
Dillibazar) runs `gpo.gov.np`, which publishes the postal code page.

**What the page showed, by date** (Wayback CDX for
`gpo.gov.np/pages/postal-code-1259614658/`, read 2026-11-07):

| Snapshot | Content |
|---|---|
| 2025-05-17 | Embeds `postalcode_8xt2owp.pdf`, probably the 1991 list. Not archived; not read |
| 2025-09-08, 2026-01-09, 2026-07-20, 2026-10-01 | One HTML table of 838 rows. The parsed rows are byte-identical across all four snapshots |
| 2026-11-07 (live) | Embeds `UPU Postal Code English_rotated_s8zdbyq.pdf` (4,909,399 B, Last-Modified 2026-10-14). HEAD works; every GET gets an empty reply. Not read |

A second GPO file, `पोष्टल कोड_5aoxvqu.pdf` (679,701 B, Last-Modified 2025-12-02), has the
column headings "S.N., Name of Office in English, Province, Local Level Code, Office Postal
Code", according to a search index excerpt. It is refused in the same way. Not read.

**The table (verified, our parse of the archived HTML):**

- Headings: क्र. सं. (serial), प्रदेश (province), जिल्ला (district), स्थानीय तहको नाम (local
  level), वडा संख्या (ward count), कार्यालय (office), कोड (स्थानीय तह) (code, local level), कोड
  (स्थानीय तह तथा वडा समेत) (code, local level and ward).
- 753 numbered rows, one per local level. Each has a 5-digit code and a ward range written as
  `1010101 देखि 07` ("1010101 to 07"). For every row the range starts at the local code + `01`
  and ends at the ward count. The ward counts sum to **6,743**, which matches the MoFAGA count
  in [Nepal sources](nepal-sources.md).
- 84 unnumbered rows have no code. They list offices: 12 postal directorates (हुलाक
  निर्देशनालय), 70 district post offices (जिल्ला हुलाक कार्यालय) and the GPO twice.
- The "office" column names the office that serves the local level. It is "हुलाक कार्यालय,
  <local level>" for 677 rows; for the rest it is a district post office, a directorate or the
  GPO (Kathmandu Metropolitan City 30608 → गोश्वारा हुलाक कार्यालय, काठमाडौँ). The code is the
  area's, not the office's.
- The first digit of the code matches the province on all 753 rows (Koshi 1, Madhesh 2,
  Bagmati 3, Gandaki 4, Lumbini 5, Karnali 6, Sudurpashchim 7). There are 77 distinct
  three-digit district prefixes.
- A third-party transcription, `Forgesaroj/nepal-reference-data` (MIT code, retrieved
  2026-09-27, no data licence asserted), matches the table on all 753 codes, names and ward
  counts.

**Relation to the CBS codes.** Our earlier note recorded the CBS codes for Phaktanglung
(10101), Kathmandu Metropolitan City (30608) and Kirtipur (30609); the table has the same codes.
A full 753-row comparison with the CBS list was not possible today because the Open Data Nepal
dataset page returns 404. **Partly verified:** 3 of 753 codes, plus the matching structure.

**Press** (secondary): The Himalayan Times (2026-04-29) and Clickmandu (2026-06-21) report an
upgrade "from five digits to seven digits". The Rising Nepal (2026-08-14) says the department
"introduced a seven-digit postal code system". Khabarhub (2026-05-25) says "work is also
underway to upgrade postal codes under the Postal Internal Tracking System (PITS)". The press
calls the scheme seven digits; the table shows both lengths. Five digits identify the local
level and seven the ward.

**The 1991 scheme** (secondary, Wikipedia; UPU): 5 digits, introduced November 1991. The first
two digits are the district, the last three the post office or APO area. The UPU addressing
sheet for Nepal (dated 06/2012) still shows "5 digits to the right of the locality name"
(`KATHMANDU 44601`). Wikipedia's banner calls these codes "outdated and not in use anymore",
citing no source. **The legal end date is unverified.** We extracted 917 distinct 5-digit
legacy codes from the Wikipedia table (for this measurement only; that table is CC BY-SA and is
not a source). **67 of them are also new local-level codes** (10101, 10102, …), with different
meanings. Wikidata has 337 Nepal items with P281 "postal code" (CC0; 2026-11-07); they are
probably 1991 codes (not checked).

**Licence.** Neither the page nor the archived table states a licence or copyright. A code is a
fact and the scheme is a rule: 5-digit = CBS local code, 7-digit = that code + a 2-digit ward
number. So the codes can be **derived** from the CBS codes (CC BY, Open Data Nepal) and the
ward counts, then checked against the GPO table, without copying the list. This is a
recommendation, not legal advice; Q-009 already asks the department for a statement.

### 3.2 India

- **PIN:** 6 digits and hierarchical. The first digit is the region, the first two the circle,
  the first three the sorting district (this layout is well known but was **not re-checked
  today**).
- **All India Pincode Directory** (Department of Posts, data.gov.in): fields circlename,
  regionname, divisionname, officename, pincode, officetype, delivery, district, statename,
  latitude, longitude. Published 2020-12-04; the page changed on 2026-08-10. The footer licence
  is the Government Open Data License – India (GODL); see
  [source licences](source-licences.md). **The CSV download returned 403 to us**: data.gov.in
  asks for a download form, and we send no email address. The maintainer must download it
  manually.
- **GeoNames IN** (CC BY 4.0, 2026-11-07): 155,570 rows (post offices) under **19,238 PINs**,
  8.09 rows per PIN. It is used below as a stand-in for the directory's size. That the directory
  has about the same counts is **unverified**.
- **PIN boundaries:** AIKosh lists "All India Pincode Boundary Geo JSON" and "Delivery Post
  Office Pincode Boundary" under the Open Government License – India. Search excerpts say the
  data.gov.in catalog page now shows no result and a DataMeet list post reports 404s. The
  DataMeet GitHub mirror is CC BY-SA 2.5 IN, which neither of our builds can take.
  **Unverified.**

### 3.3 South Asia

GeoNames postal covers India, Pakistan (2,560 codes), Sri Lanka (1,833) and Bangladesh
(1,349). It has no file for Nepal, Bhutan, the Maldives or Afghanistan. Bhutan and the Maldives
have UPU addressing sheets (`btnEn.pdf` exists); their national lists were not researched here.

### 3.4 USA

- **ZCTA 2026** (Census Bureau): the 2026 Gazetteer has 33,791 ZCTAs with an interior point and
  land and water area (`2026_Gaz_zcta_national.zip`, file dated 2026-09-08). Polygons are
  `tl_2026_us_zcta520.zip` (505 MB, 2026-10-06). US federal works are public domain
  (17 U.S.C. § 105; the Census statement is still on the unverified list in
  [source licences](source-licences.md)). A ZCTA approximates a ZIP area built from census
  blocks; it is not a USPS ZIP code.
- **GeoNames US:** 41,488 ZIP codes, including PO-box and unique ZIPs that have no ZCTA.
- **USPS** sells its ZIP and ZIP+4 files; none is open. ZIP+4 is out of scope.

### 3.5 The world

**GeoNames postal** (`allCountries.zip`, 19,977,538 B, built 2026-11-07, SHA-256
`e080132735a81af964bb2aeced113b436ca9d5c1e23cb036dbfa11de59f98a39`):

- CC BY 4.0, "provided as is". Credit GeoNames; a link to www.geonames.org is enough (readme).
  Countries are added "when the national postal service starts publishing data under a
  compatible license".
- 123 countries, 1,836,364 rows and 1,084,376 distinct (country, code) pairs. In this file GB,
  CA and NL have only their outward or first part. The full files hold GB 1,839,678 rows
  (Royal Mail notice: "Contains Royal Mail data © Royal Mail copyright and database right
  2022"), CA 900,676 rows and NL 468,809 rows.
- Truncated for copyright: CL, IE and MT (first part only), CN (first digits, ending 00), AR
  (first 5 positions), BR (major codes only).
- Coordinate accuracy codes over all rows: 4 (from a GeoNames feature) 697,959; 6 (centroid of
  addresses or shape) 329,599; 3: 325,856; blank 282,638; 1 (estimated) 142,060; 5: 52,682;
  2: 5,570. Many points are inferred, so a postal point is a hint, not a location.
- Rows per code (records per key) vary widely: PE 36.3, HR 21.9, UY 16.1, TR 12.3, LT 10.8,
  AR 10.3, ID 9.8, IN 8.1, while many countries are 1.0.

**OSM** (taginfo, 2026-11-07): `postal_code` on 1,266,426 objects (102,265 relations);
`boundary=postal_code` on 33,463 relations and 26,045 ways; `addr:postcode` on 119,470,837
objects. ODbL, so main only.

**OpenAddresses:** postcodes on address points, with a licence per source. Only sources with
CC0, public-domain or CC BY terms can go into lite; ODbL sources go into main; share-alike and
non-commercial sources go into neither. The per-source licences were not checked here.

**Wikidata** P281 "postal code": 2,368,862 statements (2026-11-07). CC0 (both builds), with
the lite filter on OSM-imported statements ([source licences](source-licences.md)). Useful for
place → code links and legacy codes; not a complete code list.

### 3.6 Sources per build

| Country / layer | Main (ODbL) | Lite (CC BY 4.0) |
|---|---|---|
| Nepal 2025 (5 and 7 digits) | derived: CBS code + ward, checked against the GPO table | same |
| Nepal 1991 (retired) | Wikidata P281; the GPO's old list if obtained and its terms allow | Wikidata P281 |
| India PIN, post offices | India Post directory (GODL); GeoNames IN as fallback | same |
| India PIN areas | PIN boundaries if GODL is confirmed; else none | same |
| Pakistan, Sri Lanka, Bangladesh | GeoNames | GeoNames |
| USA | ZCTA 2026 areas and polygons; GeoNames ZIP points | same |
| Rest of the world | GeoNames (full GB, CA, NL files); OSM `boundary=postal_code` polygons; OSM `postal_code` | GeoNames; Wikidata P281 |
| Former codes | snapshot diffs of the above (India monthly, ZCTA vintages, GeoNames) | same |

### 3.7 Scheme diversity the format must handle

From the GeoNames shapes (9 = digit, A = letter) and the national sources:

| Case | Examples | What the format needs |
|---|---|---|
| Fixed-length numeric | US `99999`, IN `999999`, DE `99999`, AU `9999`, CL `9999999` | strings, not integers: leading zeros (US `00601`) |
| Two lengths in one scheme, nested | NP `30608` (local level) and `3060801` (ward) | prefix = parent; kind per record |
| Separator inside | PT `9999-999`, JP `999-9999`, PL `99-999`, SE and CZ `999 99`, BR `99999-999` | strip it in the key, rebuild it for display |
| Alphanumeric, fixed | CA `A9A 9A9`, NL `9999 AA`, IE Eircode `A99 A9A9` | upper-case; strip spaces |
| Alphanumeric, variable outward | GB `A9 9AA` to `AA9A 9AA` | keep one canonical space before the 3-character inward part, so `B1 1AA` and `B11 1AA` stay apart in prefix search |
| Country prefix | LV `LV-9999`, MD `MD-9999`, LU `L-9999`, AZ `AZ 9999` | accept the code with or without the prefix |
| Suffix words | FR `75021 CEDEX 01`, RE/GP/MQ `99999 CEDEX` | allowed inside a code; part of the key |
| Two-part numeric | AE `99999 99999` in GeoNames | keep as given; meaning **unverified** |
| Hierarchical prefixes | IN region/circle/district; US ZIP3; GB area/district/sector; NP province/district/local/ward | prefix range = one subtree of the automaton |
| One code, many places | IN 8.1 per PIN, PE 36 | a list of records per key |
| One place, many codes | large cities; a Nepal local level with its wards | reverse index place → records |
| Area or point | NP (areas = admin units), US ZCTA (polygons), GB (points), IN (post offices) | `kind` per record |
| Retired codes, redirects | NP 1991 → 2025; IN monthly changes; ZCTA vintages | validity per scheme and per record; redirect to a key |
| Same string, different schemes | NP `10101` in 1991 and 2025 (67 cases) | the scheme in the key |

**Canonical normalisation, as data.** Each scheme has an entry in
`data/countries/<cc>/postal.yaml`, with its source:

```yaml
schemes:
  - id: np                     # key prefix; ASCII, lower case, unique worldwide
    authority: Department of Postal Services (GPO)
    valid_from: 2025-09-08     # first seen published; legal date unverified
    alphabet: digits
    lengths: [5, 7]            # 5 local level, 7 ward
    levels: [1, 3, 5, 7]       # meaningful prefixes: province, district, local level, ward
    aligned_to: admin          # code ↔ admin unit (5 → local level, 7 → ward)
  - id: np-1991
    alphabet: digits
    lengths: [5]
    valid_to: 2025-09-08       # superseded; legal date unverified
  - id: gb
    alphabet: alnum
    case: upper
    strip: " "
    separator: { char: " ", before_last: 3 }
    pattern: '^[A-Z]{1,2}[0-9][0-9A-Z]? [0-9][A-Z]{2}$'
  - id: lv
    optional_prefix: "LV-"
    lengths: [4]
    display: "LV-{code}"
```

The reader applies: Unicode NFKC, then the optional prefix, then case, then strip, then the
separator rule, then the pattern check (a failure returns no match, never an error). Display
uses `display`, or the separator rule when there is none. The forge compiles these entries into
the SCHEMES column (§5), so new countries arrive by a data release, not a library release.

### 3.8 Queries the SDK needs

| Query | How |
|---|---|
| `postal(code, scheme)` → records | normalise; `fst.get(scheme 0x00 code)` → ordinal → record list |
| `postal(code)` with no scheme | try each scheme in the pack whose rules accept the input (a few per country pack); answer with every hit and its scheme |
| prefix / autocomplete | `fst.range(scheme 0x00 prefix)` → an ordinal range in code order; take the first `k` |
| place → codes | reverse index (BY_PLACE) → records → key ordinal → code (walk the automaton by ordinal, or keep the code text) |
| point → postal code | admin-aligned scheme: point → admin unit (REGION) → its codes. With postal polygons: a REGION postal layer. Otherwise: nearest postal place via PLACE_INDEX, returned with its distance and `approximate` |
| former → current | the record's `redirect`, or (many to many) the record's place → current codes |

Reverse lookup of a code from its ordinal needs an automaton walk guided by outputs. `search::fst`
does not yet offer that (it has `get`, `range`, `walk`, `check`). A visit of `range(ord, ord+1)`
through `walk` would work but is slow, so §5 proposes a stored code-text column as the
alternative, to measure.

## 4. Options compared

**Q1. Where the codes live.**

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A. Own POSTAL section on `search::fst` (D-087) | one checked FST code path; prefix ranges; any alphabet; records with kind, validity and redirect | a second automaton beside SEARCH | **chosen** |
| B. Postal codes as a SEARCH tag (tag 4) | no new section | SEARCH lists are rows by importance, with no kind, validity or redirect; postal comes under the `search` feature | rejected |
| C. Per scheme, a sorted integer array (codes in base 37, delta bit-packed) | simple binary search | measured larger (US 61.4 KB vs 49.0 KB; IN 47.2 KB vs 37.3 KB; GB 3.33 MB vs 2.95 MB); breaks on variable length; prefix only for fixed width | rejected |

**Q2. Key order.**

| Option | World automaton | World section | Verdict |
|---|---|---|---|
| scheme ID as 1 byte + code | 6,473,228 B | 23,657,652 B | limited to 255 schemes per pack |
| scheme ID as 2 bytes + code | 6,473,228 B | 23,657,652 B | needs a table to read the key |
| **scheme name + 0x00 + code** | 6,473,512 B (+284 B) | 23,657,936 B | **chosen**: keys read the same in every pack and merge across packs, for no measurable cost |
| code + 0x00 + scheme | 8,114,760 B (+25%) | 29,617,344 B (+25%) | rejected: bigger, and prefix search mixes countries |

**Q3. What a record points to.**

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| P1. Every record → a PLACES row (admin unit, settlement, or a new `postal_area` place) | one shared path: names, hierarchy, nearest and place → codes come free | unmatched post offices and points become PLACES rows (5.2 B plus names each) | **chosen**, with forge matching to existing places first |
| P2. POSTAL holds its own points and its own R-tree | no extra PLACES rows | a second spatial index; no hierarchy or names | rejected (DRY) |

**Q4. Point → postal code.**

| Option | Accuracy | Data | Verdict |
|---|---|---|---|
| Via admin REGION and hierarchy | exact where the scheme is admin-aligned | none extra | **use** for NP (5 → local level, 7 → ward) |
| REGION postal layer | exact against the source polygons | US ZCTA (both builds); OSM `boundary=postal_code` (main); IN PIN boundaries if GODL is confirmed | **use** where polygons exist |
| Nearest postal place via PLACE_INDEX | approximate; wrong near borders; GeoNames points are often inferred | none extra | fallback only, flagged `approximate`, with distance |

**Q5. Place → codes.**

| Option | Cost (world) | Verdict |
|---|---|---|
| Reverse columns in POSTAL (sorted place rows + record IDs) | 9.61 MB of 23.66 MB (GB: 3.59 MB of 9.20 MB) | **chosen**, as optional columns; the forge leaves them out when a build drops the query |
| A `postal` component on PLACES (D-076 presence + lists) | about the same: it still needs a list per place | rejected: postal data split over two sections |
| No index (scan) | 0 | rejected for the SDK default |

**Sources** (licences in [source licences](source-licences.md)):

| Option | Strengths | Weaknesses | Licence | Version | Verdict |
|---|---|---|---|---|---|
| GPO table (Nepal) | official; 753 + 6,743 codes | HTML only; replaced by a PDF; no licence | none stated | 2025-09-08 to 2026-10-01 | verification source; codes derived |
| CBS codes + ward counts | facts, CC BY | full 753 check pending | CC BY (Open Data Nepal) | 2017 scheme | **derive Nepal codes** |
| India Post directory | official, with lat/lon, monthly | download needs a form | GODL (portal footer) | page changed 2026-08-10 | **India, both builds** |
| GeoNames postal | 123 countries, daily | inferred points; truncated countries | CC BY 4.0 | 2026-11-07 | **world, both builds** |
| Census ZCTA | polygons, public domain | not USPS ZIPs | public domain | 2026 | **US areas, both builds** |
| OSM postal boundaries | polygons in many countries | coverage uneven | ODbL | weekly | **main only** |
| Wikidata P281 | CC0, links to places | incomplete | CC0 | weekly | place links and legacy codes |
| DataMeet PIN boundaries | polygons | share-alike | CC BY-SA 2.5 IN | — | rejected |

## 5. What we build ourselves, and why

**The POSTAL section** (kind 11): one column table (encoding 1), like SEARCH. Draft column IDs:

| ID | Column | Values | |
|---:|---|---|---|
| 1 | LENGTH | one value: the automaton's length in bytes | required |
| 2 | AUTOMATON | raw bytes: a `search::fst` automaton over keys `scheme 0x00 canonical-code`, zero-padded | required |
| 3 | STARTS | per key and one more: its first record, strictly increasing (ramp coding makes 1 record per key nearly free) | required |
| 4 | PLACE | per record: its PLACES row. Within a key, records are in row order | required |
| 5 | KIND | per record: 1 area, 2 post office, 3 point | optional (0 = area) |
| 6 | VALID_FROM | per record: days since 1970-01-01 + 1, 0 = the scheme's | optional |
| 7 | VALID_TO | per record: as above, 0 = the scheme's | optional |
| 8 | REDIRECT | per record: target key ordinal + 1, 0 = none | optional |
| 9 | BY_PLACE | sorted PLACES rows that have records (delta coded) | optional, with 10 |
| 10 | BY_PLACE_REC | per BY_PLACE entry: the record | optional, with 9 |
| 11 | SCHEMES | raw bytes: one entry per scheme, in key order: name, its key-ordinal range, validity and the compiled normalisation rule (§3.7) | required |

- A key is at most 255 bytes, like SEARCH. A scheme name is ASCII `[a-z0-9-]`, so the 0x00
  separator cannot occur in it. Codes are canonical UTF-8.
- `item_count` is the number of records. Open checks the frame and column lengths in O(1).
  `check` reuses `Fst::check` and then checks STARTS, row bounds, kinds, ranges and that every
  redirect is a valid ordinal: O(automaton + records).
- A scheme's validity is stored once in SCHEMES, so retiring a whole scheme (np-1991) needs no
  per-record dates. Per-record dates exist for single codes that change.
- REDIRECT holds one target. A split or merge (one old code to many new ones) has no redirect;
  the reader goes through the record's place.
- Candidate column, to measure in M3: CODE_TEXT (codes as FSST text per record), if the reverse
  walk from an ordinal (§3.8) is slow. The writer change that `search::fst` needs is a lookup of
  a key by its ordinal (follow the outputs). Its cost is O(key length × node width); measure it
  before adding a column.

**Measured sizes (estimates).** The method: a Python model of the `search::fst` writer (the four
node forms and the canonical widths from the module doc, minimal automaton with an exact
registry) and of the B1 bit-packer (FOR, ramp and delta, 128-value blocks). Place rows are
stood in for by the rank of each record's point in Hilbert order among the records themselves;
a real pack has more places, so PLACE and BY_PLACE_REC widths grow by about 1–2 bits. The Rust
writer must confirm these numbers in M3 (gate below).

| Set | Schemes | Keys | Records | Key text | Automaton | Section | B / key |
|---|---:|---:|---:|---:|---:|---:|---:|
| NP 2025 (753 + 6,743) | 1 | 7,496 | 7,496 | 58,462 B | 4,404 B | 27,804 B | 3.71 |
| NP 2025 + 1991 legacy (917) | 2 | 8,413 | 8,413 | 63,964 B | 6,628 B | 35,596 B | 4.23 |
| IN (GeoNames, post offices) | 1 | 19,238 | 155,570 | 134,666 B | 37,296 B | 574,804 B | 29.88 (3.69 / record) |
| US ZCTA 2026 | 1 | 33,791 | 33,791 | 202,746 B | 49,040 B | 157,868 B | 4.67 |
| US GeoNames ZIP | 1 | 41,488 | 41,490 | 248,928 B | 51,628 B | 196,464 B | 4.74 |
| GB full | 1 | 1,839,678 | 1,839,678 | 15.56 MB | 2.95 MB | 9.20 MB | 5.00 |
| World GeoNames (full GB, CA, NL) | 123 | 4,283,671 | 5,012,086 | 33.46 MB | 6.47 MB | 23.66 MB | 5.52 |

The automaton costs 1.5–1.6 B per key on large sets (raw key text: 7.8–8.5 B). The largest
columns are PLACE and the reverse index, both bounded by log2(places) bits per record. In the
world set, PLACE is 6.68 MB, BY_PLACE 1.55 MB and BY_PLACE_REC 8.05 MB.

**Gates for M3** (proposed):

- Nepal: 753 local and 6,743 ward codes, each equal to its admin unit's CBS code (+ ward);
  0 mismatches against the GPO table; every record's place is an admin unit of the right level.
- Every scheme's rule accepts every code of its source and refuses a deliberately bad code. A
  normaliser fuzz test: `normalise(display(c)) = c`.
- The Rust writer's sizes within 10% of the table above for NP, IN, US ZCTA and GB, recorded in
  the benchmark page; exact lookup and 10-result prefix query latencies measured.
- The 67 NP strings that collide across schemes return both records, each with its scheme.

## 6. Impact on the plan

- **Q-001:** answered (this note): 5-digit local-level and 7-digit ward codes, CBS-based; the
  1991 scheme retired. What remains is the UPU PDF content, the legal effective date and the
  licence (Q-009). The [Nepal sources](nepal-sources.md) §3.5 "inferred, unverified" structure
  is now verified from the GPO table.
- **Decisions to record:** (1) the POSTAL key is `scheme name 0x00 canonical code`, scheme
  first; (2) normalisation rules are per-scheme data in `data/countries/<cc>/postal.yaml`,
  compiled into the pack's SCHEMES column; (3) every postal record links to a PLACES row; (4)
  point → postal follows the order admin-aligned, then postal REGION layer, then nearest
  (approximate); (5) the sources per build in §3.6; (6) the reverse index is optional columns.
- **Pack format:** replace the POSTAL row's `fst` codec with "column table" and add a POSTAL
  subsection from §5 when M3 implements it.
- **Schema:** the scheme IDs `np` and `np-1991` replace `np-5` and `np-7`: the two lengths are
  one scheme, and the 1991 codes, also five digits, are a different one. Kinds stay area, post
  office and point.
- **`search::fst`:** add a lookup of a key by its ordinal, if measured fast enough.
- **Maintainer tasks:** download the India Post directory (form); download the GPO "UPU Postal
  Code English" PDF by hand and compare it with the table; ask the department for a licence
  statement (Q-009).

## 7. Risks and open items

- The UPU PDF may differ from the table (for example English names or post office codes). It
  is **unread**.
- The legal status of the 1991 codes, and the date the new scheme took effect, are
  **unverified**. `valid_from` uses the first archived publication (2025-09-08) until then.
- The Nepal codes have no stated licence. Deriving them from CBS codes is our position, not
  legal advice.
- Canada's full postcode file in GeoNames: its provenance is not documented. Canada Post has
  sued over a crowd-sourced postcode database before (Geocoder.ca, 2012; outcome not checked
  today). Review before shipping CA full codes in either build. **Unverified.**
- GB full is Code-Point Open data (OGL v3) passed through GeoNames' CC BY 4.0 and carries the
  Royal Mail notice. Carry that notice in the credits.
- GeoNames postal points are often inferred (accuracy 1 or blank on 23% of rows), so
  nearest-postal answers must be flagged approximate.
- India PIN boundary licence and availability: **unverified**.
- Size estimates come from a Python model of the format, not the Rust writer.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| GPO postal code table | identical snapshots 2025-09-08 to 2026-10-01 | 2026-11-07 | web.archive.org (`id_` raw snapshots) |
| GPO "UPU Postal Code English" PDF | Last-Modified 2026-10-14, 4,909,399 B, ETag `380b876a…` | 2026-11-07 | HEAD only |
| GPO `पोष्टल कोड_5aoxvqu.pdf` | Last-Modified 2025-12-02, 679,701 B | 2026-11-07 | HEAD only |
| UPU addressing sheet, Nepal | 06/2012 (PDF created 2020-06-08) | 2026-11-07 | upu.int |
| nepal-reference-data | pushed 2026-09-27 | 2026-11-07 | GitHub API |
| GeoNames postal `allCountries.zip` | built 2026-11-07, SHA-256 `e0801327…f98a39` | 2026-11-07 | download.geonames.org |
| GeoNames `GB_full`, `CA_full`, `NL_full` | built 2026-11-07 | 2026-11-07 | download.geonames.org |
| Census ZCTA Gazetteer | 2026, SHA-256 `f1e9046b…c54c` | 2026-11-07 | www2.census.gov |
| TIGER ZCTA520 | TIGER2026, 2026-10-06, 505 MB | 2026-11-07 | www2.census.gov (listing) |
| India Post pincode directory | page changed 2026-08-10 | 2026-11-07 | data.gov.in page metadata |
| OSM tag counts | data until 2026-11-07 | 2026-11-07 | taginfo API |
| Wikidata P281 | 2,368,862 statements; 337 for Nepal | 2026-11-07 | query.wikidata.org |

## 9. Sources

- [Postal Code Of Nepal](https://gpo.gov.np/pages/postal-code-1259614658/), General Post Office,
  Department of Postal Services; live page and Internet Archive snapshots
  [2025-09-08](https://web.archive.org/web/20250908055856/https://www.gpo.gov.np/pages/postal-code-1259614658/)
  and
  [2026-10-01](https://web.archive.org/web/20261001113959/https://gpo.gov.np/pages/postal-code-1259614658/),
  accessed 2026-11-07.
- [UPU addressing: Nepal](https://www.upu.int/UPU/media/upu/PostalEntitiesFiles/addressingUnit/nplEn.pdf),
  Universal Postal Union, 06/2012, accessed 2026-11-07.
- [Forgesaroj/nepal-reference-data](https://github.com/Forgesaroj/nepal-reference-data),
  third-party transcription, accessed 2026-11-07.
- [Postal codes in Nepal](https://en.wikipedia.org/wiki/Postal_codes_in_Nepal), Wikipedia,
  accessed 2026-11-07 (secondary).
- [From stamps to state services: Postal Office gets a new role](https://risingnepaldaily.com/news/84979),
  The Rising Nepal, 2026-08-14.
- [Govt courier service expands to 60 districts across Nepal](https://english.khabarhub.com/?p=550523),
  Khabarhub, 2026-05-25.
- Himalayan Times (2026-04-29) and Clickmandu (2026-06-21), as cited in
  [Nepal sources](nepal-sources.md).
- [GeoNames postal code readme](https://download.geonames.org/export/zip/readme.txt) and
  [dump](https://download.geonames.org/export/zip/), GeoNames, accessed 2026-11-07.
- [All India Pincode Directory](https://www.data.gov.in/resource/all-india-pincode-directory-till-last-month),
  Department of Posts, Open Government Data Platform India, accessed 2026-11-07.
- [All India Pincode Boundary Geo JSON](https://aikosh.indiaai.gov.in/home/datasets/details/all_india_pincode_boundary_geo_json.html),
  AIKosh, and [DataMeet PincodeBoundary](https://github.com/datameet/PincodeBoundary),
  accessed 2026-11-07 (search excerpts only).
- [2026 Gazetteer files](https://www2.census.gov/geo/docs/maps-data/data/gazetteer/2026_Gazetteer/)
  and [TIGER2026 ZCTA520](https://www2.census.gov/geo/tiger/TIGER2026/ZCTA520/), US Census
  Bureau, accessed 2026-11-07.
- [taginfo](https://taginfo.openstreetmap.org/) API for `postal_code`, `boundary=postal_code`
  and `addr:postcode`, accessed 2026-11-07.
- [Wikidata Query Service](https://query.wikidata.org/), P281 counts, accessed 2026-11-07.
- Daciuk, Mihov, Watson and Watson, "Incremental construction of minimal acyclic finite-state
  automata", Computational Linguistics 26(1), 2000 (the writer the size model follows).
