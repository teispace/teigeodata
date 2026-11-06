---
title: "Nepal data sources"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-06
---

# Nepal data sources

## 1. Question

Which sources can teigeodata use for Nepal, the first country, from the national outline down to wards (6,743) and settlements/toles below them? For each layer (boundaries, codes, population, settlements, postal codes, elevation ground truth, names, timezone) we need to know recency, accuracy, licence, whether it may enter the main build (ODbL) and/or the lite build (CC-BY 4.0, no ODbL inputs), and whether it follows Nepal's official 2020 map with Kalapani, Lipulekh and Limpiyadhura.

This note feeds the Nepal source manifest, the licence gate for the lite build, the boundary-perspective design (Nepal view), the postal-code model and the elevation validation set.

## 2. Summary and recommendation

- **Admin skeleton (both builds): OCHA COD-AB Nepal v02** (Survey Department source, CC BY-IGO, reviewed 30 Oct 2025, files refreshed 14 Aug 2026). Provinces (7), districts (77) and 753 local levels plus 22 protected-area units. Its national area attribute is 147,652.7 km², which is consistent with the post-2020 extent (147,516 km² officially) and not the pre-2020 147,181 km². That it includes the Kalapani/Lipulekh/Limpiyadhura area is **unverified** until we check the geometry.
- **Codes (both builds): CBS geographic codes** (province 1 digit + district 2 digits within the province + local level 2 digits; Kathmandu Metropolitan City = 30608), republished by Open Data Nepal under "Creative Commons Attribution", with Devanagari names. Make this the primary key, and keep the HDX P-code (`NP0327101`-style) as a second key.
- **Wards (main only): OSM `admin_level=9`.** There are 6,744 relations in the Geofabrik Nepal extract. They are ODbL, but where they came from is undocumented. **The lite build gets no ward polygons**: no ward polygon source we found is CC-BY-compatible. Lite carries wards as codes (local-level code + 2-digit ward number) without geometry.
- **Nepal perspective:** in the main build, build the national outline as the OSM Nepal relation joined with OSM's `boundary=disputed` relation 13559363 ("Extent of Nepalese Claims at Kalapani Territory", `claimed_by=NP`). Check it against the Survey Department 2020 `local_unit` shapefile. For lite, use COD-AB if the geometry check confirms it contains the claim. Otherwise lite needs its own CC-BY claim polygon, and none was found.
- **Population:** the Census 2021 local-level and ward tables from NSO. Ward-level results exist (the census portal has a ward report section), but **the licence and the file format are unverified**: NSO sites refused connections from our network. HDX COD-PS (CC BY-IGO) only goes down to districts, using 2023 projections.
- **Postal codes:** the official General Post Office page now serves a single PDF, "UPU Postal Code English" (last modified 14 Oct 2026). Ministry and press reports from Apr–Jun 2026 say codes moved from 5 to 7 digits "to align with the federal structure" and that PITS (Postal Internal Tracking System) was upgraded. Third-party lists, plus the match with the CBS code 30608, suggest the new 5-digit local-level code *is* the CBS code and the 7-digit code adds a 2-digit ward. **Unverified** against the PDF, which we could not download intact. No machine-readable official list and no licence were found.
- **Settlements:** OSM is the only large current source (about 14k hamlets, 5.3k villages, 1.2k neighbourhoods and 1.2k localities). No open official gazetteer exists. The Survey Department's 1990s topographic settlement layer is on HDX under unclear terms.
- **Elevation truth:** Survey Department trig points and benchmarks are sold, not open. Use the Everest height 8,848.86 m (8 Dec 2020 joint announcement), OSM `ele` tags (about 9.1k) and our own DEM checks. No open control-point set was found.
- **Timezone:** tzdb confirms `Asia/Kathmandu` was LMT +5:41:16 until 1920, +05:30 until 1986, and +05:45 after that.

**Excluded:** Kosi (CC BY-NC-SA 4.0, non-commercial); nepalboundariespy as a data source (the code is MIT but the data has no stated provenance); geoBoundaries NPL ADM2 (75 pre-2015 districts, 2006).

## 3. Findings

### 3.1 Admin boundaries

**Survey Department (DoS, Ministry of Land Management).** DoS is the national mapping organisation. In May 2020 it released the new political and administrative map: 147,516 km², 335 km² more than before, covering Limpiyadhura, Lipulekh and Kalapani, with 753 local units. It also published a `local_unit` shapefile. The original download URL (`dos.gov.np/download/news-events/shapefilegis-data-for-political-and-administrative-map-of-nepal`) now returns 404. Open Data Nepal mirrors the files. We inspected the small sidecar files only:
- `local_unit.dbf` has 777 records with the fields `STATE_CODE, DISTRICT, GaPa_NaPa, Type_GN, Province`. It carries no codes, no ward field and no Devanagari names. The 777 records are presumably the 753 local units plus protected areas and lakes (**unverified**).
- The `.prj` is `GEOGCS["NepalDD", DATUM["D_Everest_Bangladesh"], SPHEROID["Everest_Adjustment_1937"]]`, so the data are geographic coordinates on the Everest datum, **not WGS84**. A datum shift is needed. Everest 1830/1937 differs from WGS84 by tens to hundreds of metres, so applying no shift would be a gross error.
- The `.shp.xml` processing history is dated 2020-05-23, three days after the map launch. That supports this being the 2020-map dataset (inclusion of the claim is **likely, unverified**).
- Ward-level DoS data is **sold, not published**. An OSM community member (forum, 9 Jul 2026) says ward data must be bought at the Survey Department, Min Bhawan, and cannot be shared. DoS has historically priced data case by case; administrative boundaries and 1:1,000,000 data were later made free, per the NJG foreword. We found no current written licence or terms of use for DoS digital data. This is an open item.

**Ministry of Federal Affairs and General Administration (MoFAGA).** MoFAGA is the authority for local-level names and the ward counts per local level. geoBoundaries cites "Open Data Nepal, MoFAGA" as the source of its ADM3. We did not find a MoFAGA machine-readable code list or a licence for one.

**HDX / OCHA COD-AB Nepal (`cod-ab-npl`).** We read the CKAN API on 2026-11-06:
- Licence: **CC BY-IGO**. Source: "Survey Department of Nepal (ngiip.gov.np), UN Resident Coordinators Office in Nepal". It is a "cod-enhanced" dataset at version 02.
- Levels: Admin1 7, Admin2 77, Admin3 775. The Admin3 type digit in the P-code breaks down as 6 metropolitan, 11 sub-metropolitan, 276 municipalities, 460 rural municipalities (= 753), plus 22 protected-area or lake slivers (type `5`, e.g. Koshi Tappu, Chitawan, Khaptad, Shuklaphanta).
- Dates: boundaries created and last edited 1 Jan 2024; valid from 14 Mar 2024; reviewed 30 Oct 2025; resources last modified 14 Aug 2026; annual update cycle.
- P-code pattern: `NP` + province (2) + national district serial (2) + type (1) + serial (2), e.g. `NP0775401` = Byas, Darchula. The sheets also include edge-matched (`_em`) variants and admin lines and points.
- Admin0 `area_sqkm` = 147,652.72 (edge-matched 147,609.53). Darchula/Byas `area_sqkm` = 837.66. Whether the polygon contains the claim area is **unverified**: we did not download the 26–60 MB geometry.
- Accuracy: derived from DoS data. Methodology history: ITOS QA in 2018, MapAction adapted the boundaries "to reflect new Nepali policy" in 2016, Admin1 names updated in Nov 2020, level renaming in Mar 2023.
- There are no ward polygons (Admin4) in this dataset.

**Kosi.** An open-access panel that links admin, survey and geospatial data. Each Kosid ties to a polygon aligned with one of the 6,743 current wards plus 12 national parks and other protected areas. It maps current wards to the pre-2017 geography (36,020 wards in 3,973 VDCs and municipalities) and to constituencies. Version 1.0 (2026). Download is via a Google Form. Licence **CC BY-NC-SA 4.0**. The page gives no Kosid format and does not mention Kalapani. Verdict: unusable in both builds (non-commercial, share-alike). The crosswalk is still useful as a reference to check our own work by hand, provided nothing is copied.

**nepalboundariespy.** Version 0.1.0, uploaded to PyPI on 30 May 2026, author "BC Lalit", repo `github.com/Lalitgis/nepalboundariespy`. MIT licence for the package. It ships country, province, district, municipality and ward GeoJSON (6,743 wards). The README does not say where the geometry comes from: it lists CBS, HDX and OSM as "related resources" and says users convert "raw boundary files" themselves. Verdict: do not use as a data source. MIT on the code says nothing about the rights in the ward geometry.

**Open Data Nepal (Open Knowledge Nepal).** We read its CKAN API at `api.opendatanepal.com`:
- "New political and administrative boundaries Shapefile of Nepal": republished from DoS and marked **CC BY-SA**. Re-harvested 18 Jul 2025, originally created 5 Jun 2020.
- "Administrative boundaries geographic data of 753 municipalities" (author Sagar Ghimire): **CC BY-SA**.
- "Geographical Codes for States, Districts and Local Bodies" (harvested from CBS): **Creative Commons Attribution**.
- The site footer says "All content is licensed under a Creative Commons Attribution 4.0 International License".
- Caveat: Open Data Nepal is a civil-society mirror. Its licence label on DoS geometry is its own assertion, not a DoS grant (**unverified** whether DoS permits it). CC BY-SA cannot be mixed into the CC-BY lite build, and its share-alike terms are awkward to combine with ODbL in main.

**OSM.** The wiki (Nepal/Boundaries, last edited Feb 2021) lists admin_level 2 = nation, 3 = provinces, 6 = districts, 7 = municipalities, 9 = wards. Levels 4, 5 and 8 (development regions, zones, VDCs) were in use until 2015. Live tag counts from the Geofabrik Nepal taginfo (data until 2026-11-05; the extract includes some cross-border area):

| admin_level | relations | note |
|---|---|---|
| 2 | 5 | Nepal plus neighbours touching the extract |
| 3 | 0 | wiki value is no longer used |
| 4 | 14 | provinces appear to have moved to level 4 (7 Nepal + neighbouring states?) **unverified** |
| 5 | 23 | unclear, needs inspection |
| 6 | 146 | 77 districts plus cross-border districts **unverified** |
| 7 | 751 | close to 753 local levels |
| 9 | 6,744 | wards (6,743 expected) |

- The ward wiki page says wards are tagged `name=<Municipality>-NN` and `ward=N`, with no `ref`. The source of the ward geometry is not documented anywhere we found. That is a provenance risk: if the wards were traced from purchased DoS data, they could be challenged.
- Disputed area: OSM follows the "on the ground" rule. The Kalapani forum thread (May 2024) shows edit wars. Relation 13559363 is `boundary=disputed`, `claimed_by=NP`, `disputed_by=IN`, name "Extent of Nepalese Claims at Kalapani Territory". It is the cleanest ODbL source for the Nepal-perspective claim polygon.
- The Nepal relation 184633 carries `timezone=Asia/Kathmandu` and `population=29519114`.

**geoBoundaries NPL (gbOpen, current API):**

| Layer | Licence | Source | Year represented | Units |
|---|---|---|---|---|
| ADM0 | CC BY 4.0 | geoBoundaries, Open Data Nepal | 2019 | 1 |
| ADM1 | CC BY 3.0 IGO | Survey Department, OCHA FISS | 2020 | 7 |
| ADM2 | Public Domain | geoBoundaries, Wikimedia Commons | 2006 | 75 (pre-2015 districts, obsolete) |
| ADM3 | CC BY-SA 4.0 | Open Data Nepal, MoFAGA | 2019 | 774 |
| ADM4 | none | | | |

ADM0 is dated 2019, which predates the 2020 map, so it probably excludes the claim (**unverified**). geoBoundaries adds nothing over COD-AB except a CC BY 4.0 ADM0, and that one is probably the wrong perspective.

### 3.2 Official codes

- **CBS geographic codes** (Open Data Nepal mirror, harvested from CBS): 77 districts and 753 local units, with English and Devanagari names.
  - District code = province digit + 2-digit serial within the province (Taplejung 101, Kathmandu 306).
  - Local code = district code + 2-digit serial (Phaktanlung RM 10101; Kathmandu Metropolitan City 30608; Kirtipur 30609).
  - Ordering runs north-east to south-west within each unit (CBS/NPC coding methodology, Dec 2017).
  - The NSO 2021 census microdata uses its own district variable (`DIST`); whether it is 1–77 national serials is **unverified**.
- **HDX P-codes** use a different scheme (national district serial plus type digit). We will need a crosswalk between them, built from names and checked by hand.
- **Wards** are numbered 1..N within each local level. N per local level comes from MoFAGA and the Local Level Restructuring Commission (LLRC). The CBS file has no ward rows, so the ward list has to come from OSM (main), Census 2021 ward tables, or MoFAGA.
- **Old VDC crosswalk (2017):** the 3,157 VDCs and 217 municipalities merged into 744 local levels in March 2017, later 753 (the exact sequence is **unverified**). The authoritative mapping is in the LLRC report and the Nepal Gazette notice. We found no open machine-readable copy. Kosi has one, but it is NC. The Wikipedia per-VDC articles record the mappings one by one.

### 3.3 Population: Census 2021

- The 12th National Population and Housing Census 2021 has final results (2023). The NSO microdata catalogue (NPL-NSO-NPHC-2021-v01) says it gives information "up to the ward level", but microdata access is "not available".
- The results portal `censusnepal.cbs.gov.np/results` has a **ward report** download section (`/results/downloads/ward`). Its formats (PDF vs XLSX/CSV) are **unverified**: the server refused connections from our network on 2026-11-06.
- The NSO CKAN portal `data.nsonepal.gov.np` has a "Ward" dataset with 10 XLSX/CSV resources (household tables; last updated 28 Nov 2024, per the search snippet). Its licence field is **unverified** for the same reason.
- Settlement-level (tole) census counts: no evidence they are published.
- HDX COD-PS Nepal: CC BY-IGO, admin levels 0–2, sex- and age-disaggregated **2023 projections**, last modified Oct 2024. The admin2 level there is the district, so it is too coarse for local levels.
- Open Data Nepal mirrors the 2021 preliminary results under CC BY-SA, which is not usable for lite.
- Licence position: Nepal government statistics carry no clear open licence that we could find. Aggregate counts are facts, but we should still record an NSO terms check before shipping. The Statistics Act 2079 (2022) may set terms (**unverified**).

### 3.4 Settlements and toles

OSM `place=*` in the Geofabrik Nepal taginfo (2026-11-05):

| Tag | Total | Nodes |
|---|---|---|
| hamlet | 14,054 | 13,854 |
| village | 5,253 | 3,894 |
| locality | 1,188 | 1,173 |
| neighbourhood | 1,171 | 1,139 |
| isolated_dwelling | 604 | 577 |
| suburb | 463 | 462 |
| town | 304 | 283 |
| city | 28 | 28 |
| quarter | 6 | 4 |

- `name:ne` is on 25,315 objects and `name:en` on 38,230. The counts include some cross-border features.
- **Official gazetteer:** none found. DoS keeps a settlement database from its 1:25,000 (Terai, mid-hills) and 1:50,000 (mountains) topographic series of the 1990s. HDX "Settlements in Nepal" (2015, "Other" licence, OCHA humanitarianresponse terms) is that layer merged sheet by sheet. Its terms are unclear, so it is not usable without DoS permission.
- The UN UNGEGN list of names databases does not include Nepal, and DoS authors have called for a national Geographical Names Authority. No such authority is known to exist (**unverified**).
- **Other open lists:**
  - GeoNames NP dump: CC BY 4.0, 3.46 MB zip, refreshed 6 Nov 2026. It credits NGIIP as a source, so it is fine for lite.
  - Who's On First NPL admin: CC BY on HDX.
  - HOT `hotosm_npl_populated_places`: ODbL, an OSM derivative.
  - OSGeo Nepal settlements built from Overture (HDX, "Other" licence; check the per-source Overture licences).
  - GHSL: CC BY. It is a built-up and population raster useful for population sanity checks, not names.

### 3.5 Postal codes

- **1991 scheme:** 5 digits. The first 2 digits are the district and the last 3 the post office or area (Kathmandu 44600). The Department of Posts introduced it in Nov 1991. It is widely reproduced and still accepted by foreign address databases.
- **2025 federal-aligned revision:** Wikipedia's banner says the codes were "updated in 2025 to align with the federal structure". It points to the GPO page `gpo.gov.np/pages/postal-code-1259614658/`. That page, operated by the General Post Office under the Department of Postal Services and the Ministry of Communication and IT, now embeds a single PDF, `UPU Postal Code English_rotated_s8zdbyq.pdf` on `giwmscdnone.gov.np`: 4.9 MB, Last-Modified 14 Oct 2026. The download was cut off at 130 KB twice, so **we have not read its contents**.
- **7 digits and PITS:**
  - The Himalayan Times (29 Apr 2026), reporting the ICT Ministry's one-month progress: "The postal code system has also been upgraded from five digits to seven digits to align with the federal structure."
  - Clickmandu (21 Jun 2026): the department "upgraded the Postal Internal Tracking System (PITS) and transitioned the postal code system to a more efficient seven-digit format."
  - Kantipur and Onlinekhabar (May 2026) report work to upgrade postal codes in PITS, together with a review of mail routes.
- **Structure (inferred, unverified):** third-party finder sites give Kathmandu Metropolitan City as 30608 with wards 3060801..3060832. 30608 is exactly the CBS local-level code, so the new scheme is very likely CBS local code (5) + ward (2). This must be confirmed against the GPO PDF.
- **Machine-readable list:** none official. The GPO publishes a PDF only, with no licence statement. Third-party CSVs (`datahub.io/logistics/postal-codes-np`, the finder sites) have unknown provenance. GeoNames has no NP postal file: its readme says it adds countries only when the postal service publishes under a compatible licence.
- **Recommendation:** if the inference holds, derive the 7-digit codes from CBS code + ward number, since that derivation is a fact rather than a copy of the list. Keep the 1991 codes as a `legacy` field, sourced from GeoNames-style community lists only if their licence allows. Ship the 1991 codes in main only after a licence check.

### 3.6 Elevation ground truth

- The DoS Geodetic Survey Branch maintains 1st–4th order trig points, GPS, Laplace and CORS stations, and levelling benchmarks. They are supplied for a fee "according to its own norms", and a price table exists in NJG 2024 (not read). **Not open.**
- Levelling heights are tied to Survey of India fundamental benchmarks (Birganj, Bhairahawa) relative to MSL. Horizontal control was on Everest 1830.
- **Everest 8,848.86 m:** jointly announced by Nepal and China on 8 Dec 2020 (MoFA Nepal press release). It replaces the 8,848 m figure of 1954. It is a snow-surface orthometric height; the geoid model used is **unverified**.
- We found no official open list of peak heights. The Nepal Mountaineering Association and the Ministry of Culture, Tourism and Civil Aviation keep permit-peak tables, with licence and quality unverified.
- OSM `ele` is on about 9,103 objects in Nepal (mostly peaks and passes, with mixed provenance).
- Implication: our P90 ≤ 50 m vertical target has to be validated against independent points we can publish. Candidates: OSM summit `ele` values with `source` tags; published geodetic papers (NJG) that list individual control points; the Everest figure. The trig network stays a private check only if we buy it, and results derived from it could not be redistributed.

### 3.7 Names

- **Official romanization:** DoS developed a Nepali transliteration system with diacritics (ā, ī, ṭ, ḍ, ṇ, ṣ, ṁ; anusvara rendered by the following consonant) for its 1980s–90s topographic maps (S. M. Shrestha, NJG no. 11). The UN-approved 1972 system and BGN/PCGN 1964 also exist.
  - In practice, government lists use plain-ASCII romanizations that differ from list to list. Examples: CBS "Kathmandu Metropolitian City" vs "Kathmandu"; "Sudur Paschim" (COD-AB) vs "Sudur Pashchim" (DoS) vs "Sudurpashchim"; "Chitawan" vs "Chitwan".
  - No single authoritative romanization exists. We will need a name-reconciliation table and should keep `name:en` (common), `name:ne` (Devanagari) and, optionally, a Survey-diacritic form.
- **Devanagari official names:** the CBS code list (Devanagari column for districts and local levels), the constitution schedules for provinces, Nepal Gazette notices, and MoFAGA. COD-AB `name1..3` fields are empty for Nepal (`lang`=en only). In OSM, `name:ne` is widespread (25k objects).
- **Local-language endonyms:** OSM is nearly empty: `name:new` (Nepal Bhasa) 87, `name:mai` 18, `name:lif` (Limbu) 0, `name:taj` (Tamang) 0, `name:bho` 0. Other candidates are Wikidata labels (CC0; Nepal Bhasa Wikipedia has many bot-created village articles, coverage **unverified**) and the Language Commission of Nepal reports (licence **unverified**). Treat endonyms beyond Nepali as best-effort and sparse in v1.

### 3.8 Timezone

From tzdb `asia` (main branch, release 2026f):

```
Zone	Asia/Kathmandu	5:41:16 -	LMT	1920
			5:30	-	%z	1986
			5:45	-	%z
```

That is LMT until 1920, +05:30 from 1920 to 1986, and +05:45 since 1986 (the switch was at the start of 1986), with no DST. `Asia/Katmandu` is a backward link. The task's "+05:30 until 1986" is confirmed. Pre-1920 history is local mean time.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| OCHA COD-AB Nepal | DoS-derived, 7/77/753, P-codes, refreshed 2026 | No wards; claim inclusion unverified; English names only | CC BY-IGO | v02, reviewed 2025-10-30 | **Use in main and lite** (lite subject to IGO legal note) |
| DoS 2020 `local_unit` shapefile | Official 2020 map, likely includes claim | Everest datum; no codes; no wards; no open licence from DoS | None stated; Open Data Nepal says CC BY-SA | 2020-05-23 | **Reference / QA only**; not shipped |
| DoS ward data (purchase) | Official wards | Paid; no redistribution | Proprietary | unknown | Only if a licence that allows redistribution can be negotiated |
| OSM admin_level 4/6/7/9 + disputed rel. 13559363 | Wards complete (6,744), current, claim polygon exists | Ward provenance undocumented; de facto national line | ODbL | live 2026-11-05 | **Use in main** |
| Kosi | Wards + 25-year VDC crosswalk | Non-commercial | CC BY-NC-SA 4.0 | v1.0 2026 | Reject (QA by hand only) |
| nepalboundariespy | Easy ward GeoJSON | Data provenance unknown | MIT (code) | 0.1.0, 2026-05-30 | Reject |
| Open Data Nepal boundaries | DoS mirror | Share-alike; licence is the mirror's own claim | CC BY-SA | 2025-07 harvest | Reject for lite; avoid in main |
| Open Data Nepal / CBS codes | Official codes + Devanagari names | Mirror of CBS; no wards | CC BY (as labelled) | 2017 scheme | **Use in main and lite** |
| geoBoundaries NPL | CC BY 4.0 ADM0 | ADM0 2019 (pre-claim?), ADM2 obsolete, ADM3 CC BY-SA | Mixed per layer | 2023 build | Reject (COD-AB is better) |
| NSO Census 2021 ward tables | Official ward population | Licence/format unverified; site unreachable here | Unverified | 2023 final | **Use** after terms check |
| HDX COD-PS | Clean CSV | District-level 2023 projections only | CC BY-IGO | 2024-10 | Fallback for districts |
| OSM `place=*` | ~23k settlements, Devanagari names | Uneven coverage | ODbL | live | **Use in main** |
| GeoNames NP | CC BY, credits NGIIP | Older, less accurate coordinates | CC BY 4.0 | 2026-11-06 dump | **Use in lite** for settlements, with QA |
| HDX Settlements (DoS topo) | Official topo names | 2001-era; unclear terms | "Other" | 2015 | Reject until DoS confirms |
| GPO postal PDF | Official | PDF only, no licence | None stated | 2026-10-14 | Read to confirm the scheme; derive codes |
| tzdb | Authoritative | none | Public domain | 2026f | **Use** |

## 5. What we build ourselves, and why

1. **Nepal-perspective outline and the Byas claim area.**
   - Main: OSM Nepal relation ∪ OSM disputed relation 13559363. Assign the claim area to Sudurpashchim / Darchula / Byas RM. Which Byas ward(s) cover it in Nepal's view is **unverified**, so it must be sourced.
   - Lite: use COD-AB if it contains the claim. Otherwise we have to draw a CC-BY claim polygon from public-domain or own work (e.g. tracing the published 2020 map outline is *not* allowed without rights). That is unresolved.
   - Measurement: national area within ±0.5% of 147,516 km² (geodesic), a point-in-polygon test at Kalapani (~30.22 N, 80.98 E) and at Lipulekh, and a diff against the DoS `local_unit` shapefile after the Everest→WGS84 shift. The mean boundary offset target is to be set later.
2. **Code crosswalk:** CBS code ↔ HDX P-code ↔ OSM relation ↔ Wikidata QID for all 753 local levels, joined on normalised names and checked by geometry overlap. Gate: 753/753 matched, 0 duplicates, every mismatch reviewed by hand.
3. **Ward key:** `CBS5 + ward2` (7 digits). This doubles as the expected new postal code once the GPO PDF confirms it. Gate: ward counts per local level equal the MoFAGA/census counts, summing to 6,743.
4. **Name reconciliation table** (English common form, Survey diacritic form, Devanagari), with source per field.
5. **Elevation check set** we can publish: OSM summits with sources, plus figures from published papers, with a P90 report.

## 6. Impact on the plan

- An ADR is needed on **boundary perspective**: default Nepal view, a disputed-area flag, and how claim geometry is sourced per build.
- **Lite licence gate:** add CC BY-IGO (COD-AB, COD-PS) to the allowed list with an attribution template (legal check of the IGO terms). Explicitly deny CC BY-SA, CC BY-NC-* and "Other/unknown".
- The **lite build schema** must allow admin units without geometry (wards in lite).
- A **datum-handling** step (Everest 1830/1937 → WGS84) in any DoS-derived QA pipeline.
- **Postal-code model:** fields `postal_code` (7-digit, derived), `postal_code_local` (5-digit) and `postal_code_legacy` (1991), with a validity date.
- Roadmap item: buy or request DoS ward data and the trig list for internal QA only, and request a written licence statement from DoS, NSO and the Department of Postal Services.

## 7. Risks and open items

- **Unverified:** whether COD-AB geometry contains Kalapani/Lipulekh/Limpiyadhura (area evidence points that way). Next step: download the 26 MB gdb once, in a controlled step.
- **Unverified:** whether the DoS 2020 `local_unit` includes the claim; what the 777 records are.
- **Unverified:** OSM province level (taginfo shows 0 at level 3 and 14 relations at level 4) and what the 23 level-5 relations are. An Overpass check failed on 2026-11-06.
- **Provenance risk:** where OSM ward geometry came from. If it was traced from purchased DoS data, it is a takedown risk for main.
- **Unverified:** the NSO portal licence, the census ward file formats, and settlement-level counts (NSO hosts refused connections from our network).
- **Unverified:** the content of the GPO postal PDF; whether the 7-digit code = CBS code + ward; whether PITS codes are the public postal codes; the legal status of the 1991 codes.
- **Unverified:** which Byas ward(s) cover the claim area in Nepal's official view.
- CC BY-IGO in a CC BY 4.0 product needs a legal read (attribution and the IGO dispute clause).
- No open official trig or benchmark data: the elevation accuracy claim rests on our own validation set.
- Endonyms beyond Nepali are sparse (OSM `name:new` 87, `name:mai` 18).
- Romanization differs between sources: there is a risk of duplicate entities unless the reconciliation table is complete.
- The DoS digital data licence is unknown, and the department's site has moved (old URLs 404).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| OCHA COD-AB Nepal | v02, resources modified 2026-08-14, reviewed 2025-10-30 | 2026-11-06 | HDX CKAN API `cod-ab-npl` |
| OCHA COD-PS Nepal | 2023 projections, modified 2024-10-14 | 2026-11-06 | HDX CKAN API `cod-ps-npl` |
| HDX Settlements in Nepal | 2015 upload, 2001 data | 2026-11-06 | HDX CKAN API |
| geoBoundaries NPL gbOpen | ADM0–ADM3, builds Jan–Apr 2023 | 2026-11-06 | geoboundaries.org API |
| Open Data Nepal DoS boundaries | harvested 2025-07-18 | 2026-11-06 | api.opendatanepal.com |
| CBS geographic codes (Open Data Nepal) | 77 + 753 rows | 2026-11-06 | api.opendatanepal.com |
| DoS `local_unit` sidecar files | process date 2020-05-23 | 2026-11-06 | Open Data Nepal resources (.dbf/.prj/.xml) |
| Kosi | v1.0 (2026) | 2026-11-06 | project site |
| nepalboundariespy | 0.1.0 (2026-05-30) | 2026-11-06 | PyPI JSON API |
| OSM Nepal (taginfo) | data until 2026-11-05 | 2026-11-06 | taginfo.geofabrik.de/asia:nepal |
| OSM relations 184633, 13559363 | live | 2026-11-06 | OSM API 0.6 |
| GeoNames NP dump | 2026-11-06 | 2026-11-06 | download.geonames.org |
| GPO postal code PDF | Last-Modified 2026-10-14 | 2026-11-06 | gpo.gov.np (HEAD only) |
| tzdb | 2026f (main branch) | 2026-11-06 | github.com/eggert/tz `asia` |

## 9. Sources

- [Nepal - Subnational Administrative Boundaries (COD-AB)](https://data.humdata.org/dataset/cod-ab-npl), OCHA FISS / HDX, modified 2026-08-14, accessed 2026-11-06.
- [Nepal - Subnational Population Statistics (COD-PS)](https://data.humdata.org/dataset/cod-ps-npl), OCHA / HDX, 2024-10-14, accessed 2026-11-06.
- [Settlements in Nepal](https://data.humdata.org/dataset/settlements-in-nepal), HDX, 2015, accessed 2026-11-06.
- [geoBoundaries API, NPL](https://www.geoboundaries.org/api/current/gbOpen/NPL/ADM1/), William & Mary geoLab, accessed 2026-11-06.
- [New political and administrative boundaries Shapefile of Nepal](https://opendatanepal.com/dataset/new-political-and-administrative-boundaries-shapefile-of-nepal), Open Knowledge Nepal, harvested 2025-07-18, accessed 2026-11-06.
- [Geographical Codes for States, Districts and Local Bodies](https://opendatanepal.com/dataset/geographical-codes-for-states-districts-and-local-bodies), Open Data Nepal (from CBS), accessed 2026-11-06.
- [Coding methodology (CBS/NPC)](https://nepalindata.com/media/resources/bulkuploaded/coding_methodology_Eng_Dec17.pdf), Dec 2017, via search 2026-11-06 (direct fetch 404).
- [Kosi](https://sites.google.com/view/sleshshrestha/kosi), project page, v1.0 2026, accessed 2026-11-06.
- [nepalboundariespy](https://pypi.org/project/nepalboundariespy/), PyPI, 2026-05-30, accessed 2026-11-06.
- [Nepal/Boundaries](https://wiki.openstreetmap.org/wiki/Nepal/Boundaries) and [Nepal/Boundaries/Wards](https://wiki.openstreetmap.org/wiki/Nepal/Boundaries/Wards), OSM Wiki, edited 2021-02-25 / 2025-04-17, accessed 2026-11-06.
- [India-Nepal Boundary Dispute over Kalapani area](https://community.openstreetmap.org/t/india-nepal-boundary-dispute-over-kalapani-area/113008), OSM Community, May 2024, accessed 2026-11-06.
- [Boundary problems (page 3)](https://community.openstreetmap.org/t/boundary-problems/100091?page=3), OSM Community, Jul 2026, accessed 2026-11-06.
- [OSM relation 13559363](https://www.openstreetmap.org/relation/13559363), accessed 2026-11-06.
- [OpenStreetMap Taginfo Nepal](https://taginfo.geofabrik.de/asia:nepal/), Geofabrik, data until 2026-11-05.
- [Government unveils new political map including Kalapani, Lipulekh and Limpiyadhura](https://kathmandupost.com/national/2020/05/20/government-unveils-new-political-map-including-kalapani-lipulekh-and-limpiyadhura-inside-nepal-borders), The Kathmandu Post, 2020-05-20.
- [Nepal's new map covers an area of 147,516 sq km](https://myrepublica.nagariknetwork.com/news/nepal-s-new-map-covers-an-area-of-147-516-sq-km-10-000-copies-being-printed), Republica, 2020.
- [Nepal Update, UNRCC-AP E/Conf.102/IP9](https://unstats.un.org/unsd/geoinfo/RCC/docs/rccap19/ip/E_Conf.102_IP9_Nepal_Update.pdf), Government of Nepal, UN Statistics Division (data pricing policy).
- [Transliteration system for Nepali language](https://sureshmanshrestha.blogspot.com/2020/10/transliteration-system-for-nepali.html), S. M. Shrestha (reprint of NJG no. 11), 2020-10-21.
- [Where and how to find GIS data](https://www.gyanpark.com.np/2022/05/where-and-how-to-find-gis-datashape.html), Gyanpark, 2022 (secondary, on DoS geodetic products).
- [Nepal - NPHC 2021 microdata catalogue](https://microdata.nsonepal.gov.np/index.php/catalog/124), NSO, via search 2026-11-06 (direct fetch refused).
- [Census Nepal 2021 ward report downloads](https://censusnepal.cbs.gov.np/results/downloads/ward), NSO, via search 2026-11-06 (direct fetch refused).
- [12th National Population and Housing Census 2021](https://nepal.unfpa.org/en/publications/12th-national-population-and-housing-census-2021), UNFPA Nepal.
- [Postal Code Of Nepal](https://gpo.gov.np/pages/postal-code-1259614658/), General Post Office, Department of Postal Services, accessed 2026-11-06.
- [Postal codes in Nepal](https://en.wikipedia.org/wiki/Postal_codes_in_Nepal), Wikipedia, accessed 2026-11-06 (secondary).
- [1.2 million driving licences printed as ICT Ministry unveils one-month progress](https://thehimalayantimes.com/ampArticle/1038171), The Himalayan Times, 2026-04-29.
- [Government courier initiative successfully delivers essential documents](https://english.clickmandu.com/?p=9951), Clickmandu, 2026-06-21.
- [हुलाकमार्फत सरकारी ‘कुरियर’ सेवा ६० जिल्लामा विस्तार](https://ekantipur.com/news/2026/05/25/government-courier-service-through-post-office-expanded-to-60-districts-45-26.html), Kantipur, 2026-05-25.
- [Kathmandu postal code (new 30608, old 44600)](https://nepalpostalcodes.com/kathmandu/), third-party, accessed 2026-11-06 (unverified).
- [GeoNames postal code readme](https://download.geonames.org/export/zip/readme.txt) and [NP dump](https://download.geonames.org/export/dump/NP.zip), GeoNames, accessed 2026-11-06.
- [Press release on the height of Mt. Everest](https://mofa.gov.np/?p=15900), Ministry of Foreign Affairs of Nepal, 2020-12-08.
- [tz database `asia` file](https://github.com/eggert/tz/blob/main/asia), IANA tz, release 2026f, accessed 2026-11-06.
