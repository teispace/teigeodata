---
title: "Source licence matrix"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-06
---

# Source licence matrix

## 1. Question

For every candidate input to teigeodata, what is the exact licence, what does it require, and can it go into:

- **main**: ODbL 1.0 output, with OpenStreetMap and full attribution;
- **lite**: CC-BY 4.0 output. Lite must never read ODbL, share-alike or non-commercial inputs at any step, including merge decisions. A provenance gate enforces this.

Can our derived output be redistributed? What is the current version, the update cadence, and the access method?

This note feeds the provenance gate allow-list, the `ATTRIBUTION.md` generator, the per-country component plan (Nepal, India, USA), and the ODbL distribution design for packs, search and MCP answers.

Conventions: **verified** means read on the licence page or licence file itself on 2026-11-06. **Unverified** means not confirmed from a primary page, and must not be relied on until it is. This is a research note, not legal advice. Items marked for counsel need a lawyer before the first public pack.

## 2. Summary and recommendation

1. **Main (ODbL)** can take everything except non-commercial sources (GADM, UN SALB / gbAuthoritative, SHRUG, FABDEM) and share-alike sources other than ODbL (CC BY-SA: Wikipedia text, geoBoundaries NPL ADM3, DBpedia). CC BY-SA cannot be relicensed as ODbL, so it is excluded from main as well. If it is ever needed, ship it as a separate, unmerged layer under its own licence (an ODbL "Collective Database").
2. **Lite (CC-BY 4.0)** can take: public domain and CC0 data (Natural Earth, Wikidata structured data, IANA tzdb, US federal data: TIGER/Line, Gazetteer, ZCTA, GNIS, 3DEP; NASA data: NASADEM, SRTM, ATL08), CC BY 4.0 data (GeoNames, ESA WorldCover, WorldPop, GHSL, Meta HRSL, Google Open Buildings under its CC BY option, DataMeet), CC BY 3.0 IGO data (HDX COD-AB / COD-PS), India GODL data, Microsoft buildings (CDLA-Permissive 2.0, read from Microsoft's own repo, not via Overture), and Copernicus DEM (custom free licence with pass-through notices). Per-source filtering applies to Who's On First, Overture places and addresses, and geoBoundaries gbOpen.
3. **Lite must exclude** OSM, timezone-boundary-builder (ODbL), Overture divisions, buildings, base and transportation (theme licence ODbL), Kontur Population (CC BY, but built with OSM inputs), geoBoundaries layers marked ODbL (all of India ADM2 to ADM5) or CC BY-SA (Nepal ADM3), and any Who's On First record whose properties come from `osm` or a non-commercial source.
4. **Surprises:**
   - Microsoft's Global ML Building Footprints is now **CDLA-Permissive 2.0**, not ODbL. Its own README and LICENSE say so (update of 2026-08-13). Overture still lists it under its ODbL buildings theme.
   - geoBoundaries gbOpen is **not** uniformly CC BY 4.0. Nepal: ADM0 CC BY 4.0, ADM1 CC BY 3.0 IGO, ADM2 public domain, ADM3 **CC BY-SA 4.0**. India: ADM1 CC BY 2.5 IN; ADM2 to ADM5 **ODbL** (Pathways Data / LGD). All per-layer metadata was verified via the API.
   - Who's On First carries per-source licences, including **non-commercial** ones (EuroGeographics `eg`, ICAO `icao`, Foursquare `4sq` terms) and ODbL (`osm` concordances, `fra-odp`).
   - Copernicus DEM is **not CC BY**. It has a custom EU licence that requires a fixed copyright notice, a fixed liability sentence, no endorsement, and that every subsequent user is bound by the same obligations (Art. 6(e)).
   - Lite has no open timezone polygons. timezone-boundary-builder is ODbL, so lite must build its own zones from admin polygons and the public-domain tzdb.
5. **Recommendation:** implement the gate as an allow-list keyed on (source, layer or source-prefix, licence string) rather than per dataset. Record an `upstream_lineage` flag for third-party derived products (Kontur, WorldPop, GHSL, HRSL), because the rule "never read ODbL at any step" also covers inputs that someone else derived from OSM. Treating that lineage as disqualifying is a policy decision, recorded in section 7.

## 3. Findings

### 3.1 OpenStreetMap and the ODbL

- **Licence:** ODbL 1.0 ([text](https://opendatacommons.org/licenses/odbl/1-0/)). Site documentation is CC BY-SA 2.0. Verified on [osm.org/copyright](https://www.openstreetmap.org/copyright).
- **Attribution:** must name "OpenStreetMap" and make clear that the data is under the ODbL. "© OpenStreetMap contributors" is still acceptable. For a data distribution, include the attribution plus the ODbL text or a link to it, "in a location (such as a relevant directory) where users would be likely to look for it". Geocoders, and applications that embed them, must credit OSM. Machine-learning models are attributed in their documentation. Source: [OSMF Attribution Guidelines](https://osmfoundation.org/wiki/Licence/Attribution_Guidelines), adopted 2021-06-25. Verified.
- **ODbL mechanics** (verified on the licence text):
  - *Derivative Database*: a database based on the Database, including translations, adaptations and modifications, and new databases built from Substantial parts.
  - *Produced Work*: a work (image, audiovisual material, text, sounds) resulting from using the Database or a Derivative Database.
  - *Collective Database*: the Database in unmodified form, as part of a collection of independent databases.
  - §4.3: public use of a Produced Work needs only a notice. §4.4: a Derivative Database that is Publicly Used must stay under ODbL (share-alike). §4.5: internal use is not public. §4.6: if you publicly use a Derivative Database, or a Produced Work from one, you must offer the whole Derivative Database, or a file of the alterations, in machine-readable form, free of charge if online. §6.2: repeated and systematic extraction of insubstantial parts can add up to a Substantial part.
- **Substantial guideline** ([OSMF](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Substantial_-_Guideline), endorsed 2014-06-06). Insubstantial means: fewer than 100 features, one-off; or more than 100 features chosen non-systematically on your own qualitative criteria; or features covering an area of up to 1,000 inhabitants. Repeated extractions are added together. Verified.
- **Geocoding guideline** ([OSMF](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Geocoding_-_Guideline), endorsed 2017-08-24). Verified:
  - It does **not** treat geocoding results as Produced Works: declaring them so "sits uneasily with the text of the ODbL".
  - Individual results are insubstantial extracts. They "may be stored and used together with other proprietary or third party data" provided they are not aggregated into a substantial part of OSM.
  - A collection of results counts as systematic if it is "used as a general purpose geodatabase".
  - A geocoding API built on an improved Derivative Database triggers §4.2 share-alike on that database, but not on individual results.
- **Collective Database guideline** ([OSMF](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Collective_Database_Guideline_Guideline), endorsed 2016-06-17). OSM and non-OSM data stay independent (Collective) when a given data type, within a regional cut, is either all OSM or all non-OSM. **Merging with de-duplication is explicitly outside the guideline**, and the non-OSM data "may be subject to ODbL share-alike" (verified). Main's dedup and merge therefore makes main packs Derivative Databases. That is fine, because main is ODbL anyway.
- **Access and cadence:** planet PBF updated weekly, minutely diffs, Geofabrik country extracts. Planet size is roughly 80 GB or more as PBF (**unverified**, not checked today).
- **Verdict:** main yes. Lite **no**.

### 3.2 Overture Maps

Sources: [attribution page](https://docs.overturemaps.org/attribution/) (updated 2026-05-15) and [release calendar](https://docs.overturemaps.org/release-calendar/). Latest release **2026-10-21.0**, schema v2.0.0. Releases are monthly; schema changes can only land in March, June, September and December. Verified.

| Theme | Theme licence | Sources (licence) | Lite? |
|---|---|---|---|
| divisions | **ODbL** | OSM (ODbL), geoBoundaries (CC BY 4.0), Esri Community Maps (CC BY 4.0), LINZ (CC BY 4.0) | No |
| places | no theme licence; per source | Meta, Microsoft, PinMeTo, Krick, RenderSEO, DAC, BrightQuery, Birdeye, Uberall, OpenTravel Alliance (CDLA-Permissive 2.0); Foursquare (Apache 2.0, "Copyright 2024 Foursquare Labs, Inc. All rights reserved."); AllThePlaces (CC0 1.0) | Conditional |
| buildings | **ODbL** | OSM, Microsoft ML (listed as ODbL), Google Open Buildings (CC BY 4.0), Esri (CC BY 4.0), USGS 3DEP, Qian Shi et al. (CC BY 4.0), BTN 2024 ign.es (CC BY 4.0) | No (take Google and Microsoft direct) |
| addresses | per source ("permissive open licenses") | more than 100 sources. Examples: US NAD (NAD Access and Usage License), Brazil IBGE (CC0), NZ LINZ (CC BY 4.0), Australia G-NAF (custom EULA), Germany (DL-DE) | Conditional, per source |
| base | ODbL | OSM, ESA WorldCover, ETOPO1, GLOBathy, Daylight | No |
| transportation | ODbL | OSM, TomTom | No |

Recommended attribution: "Overture Maps Foundation, overturemaps.org", plus "© OpenStreetMap contributors, Overture Maps Foundation" for OSM-derived themes. Access: GeoParquet on S3 and Azure; sizes not checked (**unverified**).

The lite condition for places and addresses: Overture's conflation and admin assignment may use ODbL divisions, for example to fill country or region fields. Whether any ODbL-derived field or decision appears in places or addresses rows is **unverified**. Lite may only read the per-source licensed fields (geometry, names, categories, source record IDs), must drop admin fields, and should prefer the upstream source direct where it exists (AllThePlaces CC0, national address files).

### 3.3 Who's On First

The format and structure are CC0. Linking back to the licence is required and credit is recommended ("Data from Who's On First. License"). See [WOF licences](https://whosonfirst.org/docs/licenses/) and the [data repo LICENSE](https://github.com/whosonfirst-data/whosonfirst-data/blob/master/LICENSE.md). Data licences are **per source**, listed in [whosonfirst-sources](https://github.com/whosonfirst/whosonfirst-sources/blob/main/sources/README.md). Verified examples:

- GeoNames `gn`: CC BY.
- Quattroshapes `qs`: CC BY 2.0.
- Wikidata `wd`: CC0.
- Wikipedia `wk`: CC BY-SA 3.0.
- DBpedia `dbp`: CC BY-SA 3.0.
- OpenStreetMap `osm`: ODbL (concordances).
- `fra-odp`: ODbL (deprecated in 2018).
- EuroGeographics `eg`: **non-commercial**.
- ICAO `icao`: personal, non-commercial.
- Foursquare `4sq`: listed as CC BY, but the terms say personal, non-commercial.

Distribution: SQLite (global admin, postcode and constituency; per country), shapefiles and bundles. Sizes and cadence are not stated (**unverified**).

Verdict: main yes, after removing NC sources and CC BY-SA properties. Lite only with a per-property `src:*` whitelist, and only after an audit showing that no NC, SA or ODbL source contributed geometry, names or hierarchy for the countries in scope (**not yet done**).

### 3.4 Wikidata and Wikipedia

- **Wikidata** structured data (main, Property, Lexeme and EntitySchema namespaces) is **CC0** ([Wikidata:Licensing](https://www.wikidata.org/wiki/Wikidata:Licensing)). Text in other namespaces is CC BY-SA 4.0. Verified. Main yes; lite yes. Provenance caveat: some Wikidata statements were imported from OSM, GeoNames or Wikipedia. CC0 is the operative licence, but the lite gate should drop statements whose reference (P143 "imported from", P248 "stated in") points to OSM (policy, section 7). Dumps are weekly JSON and RDF, over 100 GB compressed (**unverified**).
- **Wikipedia** text is **CC BY-SA 4.0**, co-licensed with the GFDL ([Wikipedia:Copyrights](https://en.wikipedia.org/wiki/Wikipedia:Copyrights)). Verified. Excluded from both builds as merged data. Titles and sitelinks reached through Wikidata are CC0 data.

### 3.5 GeoNames

**CC BY 4.0** ([about](https://www.geonames.org/about.html); postal [readme](https://download.geonames.org/export/zip/readme.txt)). Verified.

- Required credit: credit GeoNames; "a link on your website to www.geonames.org is ok".
- Postal data is provided "as is". The UK full file carries a Royal Mail notice. Some countries are truncated for copyright reasons (CL, IE, MT, CN, AR, BR).
- The postal dump has no Nepal file (`NP.zip` returns 404). It has India (`IN.zip`, 1.7 MB) and the USA (`US.zip`, 0.6 MB).
- `allCountries.zip` is 422.5 MB and the postal `allCountries.zip` is 20.0 MB. Both were regenerated on 2026-11-06, so the dumps are rebuilt daily.

Main yes; lite yes. Caveat: many postal lat/lon values are inferred by GeoNames' own algorithm, so treat them as low-confidence.

### 3.6 Administrative boundaries

- **geoBoundaries gbOpen**: the [site](https://www.geoboundaries.org/) says CC BY 4.0 and "requires an acknowledgement in any products you produce". Websites must show "geoBoundaries" with a link; apps must show it in credits or help; written works cite Runfola et al. 2020, PLoS ONE 15(4): e0231866. Licences are **per layer** and must be read from the API field `boundaryLicense`. Verified on 2026-11-06:

  | Layer | Licence | Source | Year |
  |---|---|---|---|
  | NPL ADM0 | CC BY 4.0 | geoBoundaries, Open Data Nepal | 2019 |
  | NPL ADM1 | CC BY 3.0 IGO | Survey Department of Nepal, OCHA FISS | 2020 |
  | NPL ADM2 | Public domain | geoBoundaries, Wikimedia Commons | 2006 (stale) |
  | NPL ADM3 | **CC BY-SA 4.0** | Open Data Nepal, MoFAGA | 2019 |
  | IND ADM1 | CC BY 2.5 IN | DataMeet, Election Commission | 2011 |
  | IND ADM2 to ADM5 | **ODbL 1.0** | Pathways Data, LGD / Census | 2011 to 2021 |
  | USA ADM2 | Public domain | Census MAF/TIGER | 2018 |

  The current build is 6.0.0 (2023-09-14 in the README; layers built 2023-12-12). The README describes the database as "(CC BY 4.0 / ODbL)".
- **gbAuthoritative / UN SALB**: the SALB [Terms of Use 2021](https://salb.un.org/sites/default/files/wysiwyg_uploads/docs_uploads/TermsOfUseSALB2021.pdf) limit use to **non-commercial** purposes and make it revocable. This was seen in search excerpts only; the PDF was not read in full, so it is **partly verified**. gbAuthoritative mirrors SALB. The gbAuthoritative/IND API call returned 404. Excluded from both builds.
- **GADM**: "freely available for academic use and other non-commercial use. Redistribution or commercial use is not allowed without prior permission." ([licence](https://gadm.org/license.html)). Verified. Excluded from both builds. Note: the GHSL download page links GADM files; do not pick them up through it.
- **Natural Earth**: public domain. "Crediting the authors is unnecessary"; the optional credit is "Made with Natural Earth." Comes with a disclaimer ([terms](https://www.naturalearthdata.com/about/terms-of-use/)). Verified. Both builds. Use for the world skeleton only; Natural Earth applies de facto boundary choices.
- **HDX COD-AB**: licences from the HDX CKAN API, verified on 2026-11-06:
  - NPL: **CC BY-IGO**, Survey Department of Nepal and UN RCO, modified 2026-08-14. Version 02, admin 1/2/3 with 7/77/775 units, boundaries created 2024-01-01, reviewed 2025-10-30. Download 26 MB (GDB) to 60 MB (GeoJSON).
  - BGD, LKA and PAK: also CC BY-IGO.
  - IND: no COD-AB package (API error).
  - NPL COD-PS (population, admin 0 to 2, 2023 projections, UNFPA): CC BY-IGO.
- **Is CC BY 3.0 IGO compatible with a CC BY 4.0 output?** On the [legal code](https://creativecommons.org/licenses/by/3.0/igo/legalcode):
  - Adaptations are allowed if changes are labelled (§3(b)). There is **no share-alike clause**. §4(a) forbids imposing terms that restrict the licence and does not require a Collection to be under the same licence.
  - Attribution needs the Licensor's notices, the title, a URI, and a credit for adaptations. There is a no-endorsement clause.
  - IGO-specific terms: mediation, then UNCITRAL arbitration (§8(h)), and preservation of privileges and immunities (§8(g)). These bind us, not our recipients.
  - **Verdict:** yes. Our CC BY 4.0 adaptation can include it, with its notices carried in `ATTRIBUTION.md` and changes labelled. The license text was verified. That CC BY 4.0 as the adapter's licence is acceptable follows from the absence of an SA clause; the CC FAQ section on adapter's licences was not read in full (**unverified** for that specific sentence).

### 3.7 Elevation

- **Copernicus DEM GLO-30 Public / GLO-90**: a custom EU licence ([licence PDF](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf), [collection page](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM)). Verified from the licence text:
  - Art. 4 grants reproduction, distribution, communication to the public, and adaptation, modification and combination. Art. 3: worldwide, perpetual. Art. 5: free of charge. Commercial use is not excluded.
  - Art. 6(a), unmodified, notice: "© DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA; all rights reserved."
  - Art. 6(b), adapted, notice: "produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and ESA; all rights reserved". For GLO-90 the text reads "WorldDEM™-90".
  - Art. 6(c), mandatory disclaimer: "The organisations in charge of the Copernicus programme by law or by delegation do not incur any liability for any use of the Copernicus WorldDEM-30" (or WorldDEM™-90).
  - Art. 6(d): no impression of endorsement. **Art. 6(e): subsequent users must be bound by these obligations.** Art. 7: "as is" plus an indemnity. Art. 9: termination on breach.
  - The data is a DSM, not a DTM. Latest release 2024_1 (July 2024). The page says maintenance runs "until 2026". Access: AWS open data, with the citation "Copernicus Digital Elevation Model (DEM) was accessed on DATE from https://registry.opendata.aws/copernicus-dem". Size not checked (**unverified**).
  - Main yes. Lite: allowed by the gate (not ODbL, SA or NC), but its pass-through notices sit alongside CC BY 4.0. Ship them as notices in the lite pack and have counsel confirm they are "attribution-like" rather than additional restrictions under CC BY 4.0 §2(a)(5)(B).
- **NASADEM / SRTM**: NASA data "licensed as Creative Commons Zero (CC0)" unless marked otherwise. Citation is strongly urged, and NASA material may not imply endorsement ([Earthdata data use guidance](https://www.earthdata.nasa.gov/engage/open-data-services-software-policies/data-use-guidance)). Verified. Product version (NASADEM_HGT v001) and size are **unverified**. Both builds. This is the cleanest lite DEM.
- **ALOS AW3D30**: JAXA says use is free of charge for "commercial and non-commercial purposes" under its Terms of Use, and that v4.1 (April 2024) covers the globe except Antarctica and Japan (search excerpt of the [JAXA page](https://www.eorc.jaxa.jp/ALOS/en/dataset/aw3d30/)). The credit requirement ("© JAXA", source display, no quality guarantee) comes from the 2016 terms as quoted on the OSM wiki. **Unverified for the current terms**: the [terms page](https://www.eorc.jaxa.jp/ALOS/en/aw3d30/terms.htm) did not render the text. Hold for both builds until it is read.
- **FABDEM**: **CC BY-NC-SA 4.0**, with commercial use via Fathom, according to several secondary catalogues. The University of Bristol data page timed out, so this is **unverified from primary**. One torrent mirror lists a different NC licence. Excluded from both builds either way: it is non-commercial, and the share-alike is not ODbL.
- **ICESat-2 ATL08**: NASA NSIDC DAAC. Citation is required ("As a condition of using these data, you must cite"). Version 6 (DOI 10.5067/ATLAS/ATL08.006) is retired in favour of v7 ([NSIDC](https://nsidc.org/data/atl08/versions/6)). Covered by the NASA CC0 policy. Both builds, as a validation reference rather than a shipped layer.
- **USGS 3DEP**: "free of charge and without use restrictions", public domain ([USGS](https://www.usgs.gov/3d-elevation-program/about-3dep-products-services)). Products: 1/3 arc-second seamless, and 1 m COG tiles (production since mid-2025). Access via TNM and AWS (lidar). Both builds.

### 3.8 Buildings

- **Google Open Buildings v3**: "shared under the Creative Commons Attribution (CC BY-4.0) license and the Open Data Commons Open Database License (ODbL) v1.0 license. As the user, you can pick which of the two licenses you prefer". It carries a liability disclaimer, and citing Sirko et al. 2021 (arXiv:2107.12283) is requested ([Google](https://sites.research.google/gr/open-buildings/)). Verified. It has 1.8 B detections (inference May 2023). Coverage includes India, Nepal, Bangladesh, Bhutan, Sri Lanka and the Maldives, but not Pakistan. Size: polygons 178 GB, points 48 GB. Main uses the ODbL option or CC BY; **lite uses the CC BY option**. Read it direct from Google, not via Overture.
- **Microsoft Global ML Building Footprints**: "licensed by Microsoft under the CDLA Permissive 2.0" ([repo README and LICENSE](https://github.com/microsoft/GlobalMLBuildingFootprints)). Verified. It has 1.4 B buildings from 2014 to 2024 imagery, in 30,340 tiles across 225 regions; last refresh 2026-08-13. India and Nepal were added in March 2024. CDLA-P 2.0 requires sharing the agreement text when sharing the data, and it places no restriction on Results (**the CDLA-P text itself was not re-read today**). Both builds. Overture's "ODbL" label for this source appears out of date, or applies to the copy Overture holds.

### 3.9 Population

- **GHSL (GHS-POP, GHS-BUILT)**: "Reuse of this data is authorised with proper acknowledgment of the source" ([download page](https://human-settlement.emergency.copernicus.eu/download.php)). Search excerpts say the GHSL how-to-cite page states CC BY 4.0, with the EC Reuse Notice governing (**partly verified**). The current epoch release is R2023A. Both builds, subject to the lineage question (section 7).
- **WorldPop**: "WorldPop datasets are licensed under the Creative Commons Attribution 4.0 International License" ([hub](https://hub.worldpop.org/project/categories?id=3)). Verified. Latest is **R2025A v1 "Global2"**, covering 2015 to 2030 at 100 m and 1 km. Both builds, subject to lineage: covariates historically include OSM-derived layers (**unverified** for R2025A).
- **Kontur Population**: CC BY International on HDX ([HDX](https://data.humdata.org/dataset/kontur-population-dataset)). Latest file is 2023-11-01 (`kontur_population_20231101.gpkg.gz`, 2.4 GB). Its own page lists inputs: GHSL, HRSL, Microsoft buildings, LINZ, Copernicus Global Land, **and OpenStreetMap** "to constrain" artefacts ([Kontur](https://www.kontur.io/datasets/population-dataset/)). Verified. Main yes. **Lite no**: it is built on OSM, which conflicts with the "never read ODbL at any step" rule. Whether its CC BY licence is even valid, given ODbL share-alike, is an open question.
- **Meta HRSL**: CC BY 4.0, updated quarterly on AWS. Citation: "High Resolution Population Density Maps + Demographic Estimates by CIESIN and Meta"; imagery © 2016 Maxar ([AWS registry](https://registry.opendata.aws/dataforgood-fb-hrsl/)). Verified. Both builds. Its input lineage is **unverified**.

### 3.10 Time zones

- **IANA tzdb**: "all files in the tz code and data ... are in the public domain", except three BSD-licensed code files ([LICENSE](https://github.com/eggert/tz/blob/main/LICENSE)). Latest is **2026f**, released 2026-10-27 ([IANA](https://www.iana.org/time-zones)). Releases are irregular, several a year. Both builds.
- **timezone-boundary-builder**: the code is MIT; "The outputted data is licensed under the Open Data Commons Open Database License (ODbL)" ([README](https://github.com/evansiroky/timezone-boundary-builder)). Verified. Latest release **2026f**, 2026-11-01. `timezones-with-oceans.geojson.zip` is 56 MB; `timezones-now.geojson.zip` is 25 MB. The boundaries are built from OSM. Main yes. **Lite no.**

### 3.11 Land cover

- **ESA WorldCover** 2020 v100 and 2021 v200: CC BY 4.0, "provided free of charge, without restriction of use". Map attribution: "© ESA WorldCover project [year] / Contains modified Copernicus Sentinel data ([year]) processed by ESA WorldCover consortium". Cite Zanaga et al. 2021 / 2022 (DOIs 10.5281/zenodo.5571936 and .7254221). 2,651 COG tiles, about 117 GB ([ESA](https://esa-worldcover.org/en/data-access)). Verified. Both builds.

### 3.12 India

- **GODL (Government Open Data License – India)**: Gazette of India, MeitY notification of 10 February 2017, published 13 February 2017 ([PDF](https://www.data.gov.in/sites/default/files/Gazette_Notification_OGDL.pdf)). Verified from the gazette text:
  - §3: "worldwide, royalty-free, non-exclusive license to use, adapt, publish (either in original, or in adapted and/or derivative forms) ... for all lawful commercial and non-commercial purposes".
  - §4(a): attribution statement plus DOI or URL is mandatory. §4(b): attribution for multiple datasets may sit on a separate linked page.
  - §4(c): **non-endorsement**. §4(d): no warranty. §4(e): no continuity guarantee.
  - §5 template: "[Name of Data Provider], [Year of Publication], [Name of Data], [Name of Data Repository/Website], [Version Number and/or Date of Publication (dd/mm)], [DOI / URL / URI]. Published under [Name of License]: [URL of License]."
  - §6 exemptions: personal information, non-shareable data, logos and official marks, other IP.
  - §7 termination: automatic on breach, with a 30-day cure period. The provider may revoke a dataset.
  - §8: arbitration by an arbitrator appointed by the Union law secretary. §9: Indian law.
  - No share-alike. Both builds.
- **All India Pincode Directory** (Department of Posts, data.gov.in resource `5c2f62fe-5afa-4119-a499-fec9d604d5bd`): fields are circle, region, division, office name, pincode, office type, delivery, district, state, **latitude, longitude**. Updated monthly; the last record covers up to 2026-05-24. CSV plus an API (key required). The licence is GODL according to the portal-wide footer; the per-resource licence field is empty (**partly verified**).
- **PIN code boundaries**: "All India PIN Code Boundary geo-json" and "Delivery Post Office Pincode Boundary", on data.gov.in and AIKosh, listed under "Open Government License, India" in search excerpts. The pages did not render (**unverified**). If confirmed, this is a high-value lite input.
- **DIGIPIN** (India Post grid geocode): code at [INDIAPOST-gov/digipin](https://github.com/INDIAPOST-gov/digipin). GitHub reports NOASSERTION because the `LICENSE` file is a reformatted Apache-2.0 text; the README says Apache-2.0 and `package.json` says ISC (checked 2026-11-06). Both are permissive, so it is admissible. It is an algorithm, not data: we reimplement it from the spec and credit India Post.
- **LGD (Local Government Directory)**: reports, downloads and the NAPIX API; content is "owned, updated and managed by the Panchayats". **No licence statement was found**: the terms and copyright URLs return 404. **Unverified.** Treat LGD codes as factual identifiers, but get written confirmation, or find an LGD resource on data.gov.in under GODL, before lite uses them.
- **Census 2011 PCA**: the ORGI digital library shows "Rights: Office of the Registrar General ... All Rights Reserved" on catalogue pages. Copies on data.gov.in would be under GODL (**unverified**, not located today). Use only the data.gov.in GODL copies.
- **SHRUG / Development Data Lab**: "licensed under a Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International License", "as is". SHRUG 2.2 (August 2026) ([DDL](https://www.devdatalab.org/shrug_download/)). Verified. Excluded from both builds, including its village polygons.
- **DataMeet maps**: the README says that "Unless explicitly stated, all datasets in this repository is shared under CC BY 4.0", with an example credit ([datameet/maps](https://github.com/datameet/maps)). Verified. The GitHub licence metadata says MIT (for code). Last push 2022-05-11. Both builds, but check each layer's lineage. Some layers were digitised from Census or Survey of India maps whose own licences are unclear (**unverified**). DataMeet itself says it is "not sure about the exact license" of the Survey of India external boundary it hosts (search excerpt).
- **Survey of India / National Geospatial Policy 2022**: the primary policy text was not reached today. Survey of India pages state that its maps are under copyright and its digital data is "licensed based on usage". Open Series Maps are a free PDF download behind an Aadhaar login. No open-licence grant was found (**unverified** whether NGP 2022 changed this). Exclude until a licence is found. Separate legal risk: the depiction of India's external boundary (section 7).

### 3.13 USA

- **USGS GNIS** (domestic names): USGS-produced data "are considered to be in the U.S. Public Domain"; credit is requested as "Credit: U.S. Geological Survey" ([USGS policy](https://www.usgs.gov/information-policies-and-instructions/copyrights-and-credits)). Updated every other month via the TNM Staged Products Directory. In 2021, administrative and cultural features (airports, schools, dams and so on) were removed ([BGN](https://www.usgs.gov/us-board-on-geographic-names/domestic-names)). Verified. Both builds.
- **Census TIGER/Line, Gazetteer files, ZCTA**: the 2026 TIGER/Line was released 2026-10-14, with boundaries as of 2026-01-01; annual vintages ([Census](https://www.census.gov/geographies/mapping-files/time-series/geo/tiger-line-file.html)). The 2026 Gazetteer files include ZCTAs, places and counties with representative lat/lon ([Gazetteer](https://www.census.gov/geographies/reference-files/time-series/geo/gazetteer-files.html)). The pages carry no licence statement. Public-domain status rests on 17 U.S.C. §105 and on geoBoundaries labelling the derived layer "Public Domain" (**no Census statement verified today**). Both builds. ZCTAs approximate ZIP codes; USPS ZIP data itself is not open.
- **USGS 3DEP**: see 3.7.
- **US National Address Database**: "National Address Database Access and Usage License", via Overture. The DOT page returned 403 (**unverified**).

### 3.14 Other sources checked

- **HDX COD-AB** for Bangladesh, Sri Lanka and Pakistan: CC BY-IGO (verified via API). Lite-eligible for South Asia.
- **HOT OSM exports** on HDX: ODbL. Main only.
- **OpenNepal census tables** on HDX: ODbL. Main only. **OCHA Nepal 2011 census COD**: CC0. Both builds.
- **AllThePlaces**: CC0 (via Overture). Both builds; take direct.
- **Not checked today** (**unverified**, for a follow-up note): OpenAddresses (per-source licences); Nepal Post postcode list; Nepal NSO Census 2021 portal terms; Japan GSI, Korea NGII and Indonesia BIG open data terms; Unicode CLDR (exonyms, Unicode licence); USPS.

## 4. Options compared

Compact matrix. M = main (ODbL), L = lite (CC-BY 4.0). ✓ = yes, ✗ = no, ◐ = conditional (see the notes column). "Redist." means our derived output can be redistributed under the build's licence.

| Source | Licence (verified?) | Key obligations | M | L | Redist. | Version / cadence | Access, size |
|---|---|---|---|---|---|---|---|
| OpenStreetMap | ODbL 1.0 (✓) | attribution, ODbL SA, §4.6 offer DB | ✓ | ✗ | ODbL only | weekly planet, minutely diffs | PBF ~80 GB+ (unverified) |
| Overture divisions / buildings / base / transport | ODbL (✓) | as OSM + Overture credit | ✓ | ✗ | ODbL | 2026-10-21.0, monthly | GeoParquet S3/Azure |
| Overture places | per source: CDLA-P 2.0 / Apache 2.0 / CC0 (✓) | per source; Foursquare notice | ✓ | ◐ source filter, drop admin fields | yes | monthly | GeoParquet |
| Overture addresses | per source (✓ list) | per source | ✓ | ◐ per source | yes, per source | monthly | GeoParquet |
| Who's On First | CC0 structure; per-source data (✓) | link to licence; per-source; NC sources present | ◐ drop NC/SA | ◐ src whitelist, audit | yes after filtering | rolling, "latest" | SQLite per country |
| Wikidata | CC0 (✓) | none | ✓ | ✓ (drop OSM-imported claims) | yes | weekly dumps | JSON dumps >100 GB (unverified) |
| Wikipedia text | CC BY-SA 4.0 (✓) | SA | ✗ | ✗ | no | continuous | dumps |
| GeoNames + postal | CC BY 4.0 (✓) | credit + link | ✓ | ✓ | yes | daily dumps | 422.5 MB / 20 MB zip |
| geoBoundaries gbOpen | per layer (✓) | acknowledgement + per-layer | ◐ exclude BY-SA layers | ◐ only BY / PD / IGO layers | per layer | 6.0.0 (2023) | API + GitHub zips |
| gbAuthoritative / UN SALB | NC (partly ✓) | non-commercial, revocable | ✗ | ✗ | no | n/a | n/a |
| GADM | NC, no redistribution (✓) | permission needed | ✗ | ✗ | no | n/a | n/a |
| Natural Earth | public domain (✓) | none | ✓ | ✓ | yes | irregular | small |
| HDX COD-AB / COD-PS | CC BY 3.0 IGO (✓) | credit, label changes, no endorsement | ✓ | ✓ | yes | per country (NPL 2026-08-14) | 26–60 MB (NPL) |
| Copernicus DEM GLO-30/90 | custom EU licence (✓) | fixed notice, liability sentence, no endorsement, pass-through | ✓ | ◐ counsel check | yes, with notices | 2024_1 | AWS COG (size unverified) |
| NASADEM / SRTM | NASA CC0 (✓ policy) | cite; no NASA endorsement | ✓ | ✓ | yes | static | LP DAAC |
| ALOS AW3D30 | JAXA terms, free incl. commercial (unverified) | © JAXA (unverified) | hold | hold | likely | v4.1 (2024) | JAXA portal |
| FABDEM | CC BY-NC-SA 4.0 (unverified primary) | NC, SA | ✗ | ✗ | no | v1-2 | n/a |
| ICESat-2 ATL08 | NASA CC0 + citation (✓) | cite | ✓ | ✓ | yes | v7 current | NSIDC |
| USGS 3DEP | public domain (✓) | credit requested | ✓ | ✓ | yes | rolling | TNM / AWS |
| Google Open Buildings v3 | CC BY 4.0 **or** ODbL, user's choice (✓) | credit; disclaimer | ✓ | ✓ (CC BY option) | yes | v3 (2023) | 178 GB polygons |
| Microsoft ML Buildings | CDLA-P 2.0 (✓) | include agreement text | ✓ | ✓ | yes | 2026-08-13 | 30,340 tiles |
| GHSL POP / BUILT | CC BY 4.0 / EC reuse (partly ✓) | acknowledgement | ✓ | ◐ lineage | yes | R2023A | JRC |
| WorldPop | CC BY 4.0 (✓) | credit | ✓ | ◐ lineage | yes | R2025A v1 Global2 | 100 m / 1 km GeoTIFF |
| Kontur Population | CC BY (✓), OSM input (✓) | credit | ✓ | ✗ | main only | 2023-11-01 | 2.4 GB gpkg.gz |
| Meta HRSL | CC BY 4.0 (✓) | credit; Maxar imagery | ✓ | ◐ lineage | yes | quarterly | AWS |
| timezone-boundary-builder | ODbL (✓) | as OSM | ✓ | ✗ | ODbL | 2026f (2026-11-01) | 25–56 MB zip |
| IANA tzdb | public domain (✓) | none | ✓ | ✓ | yes | 2026f (2026-10-27) | tarball |
| ESA WorldCover | CC BY 4.0 (✓) | fixed credit | ✓ | ✓ | yes | 2020 v100, 2021 v200 | ~117 GB COG |
| India GODL datasets | GODL 2017 (✓) | attribution statement, non-endorsement | ✓ | ✓ | yes | per dataset | data.gov.in |
| Pincode directory | GODL (portal footer, partly ✓) | GODL §5 | ✓ | ✓ | yes | monthly | CSV / API |
| Pincode boundaries | GODL (unverified) | GODL §5 | ◐ | ◐ | if GODL | unknown | GeoJSON |
| LGD codes | none found (unverified) | unknown | ◐ | ◐ | unknown | rolling | reports / NAPIX API |
| Census 2011 PCA | ORGI "All Rights Reserved"; GODL copies (unverified) | GODL if via data.gov.in | ◐ | ◐ | if GODL | 2011 | data.gov.in |
| SHRUG village polygons | CC BY-NC-SA 4.0 (✓) | NC, SA | ✗ | ✗ | no | 2.2 (Aug 2026) | n/a |
| DataMeet maps | CC BY 4.0 (✓) | credit | ✓ | ◐ lineage per layer | yes | 2022 | GitHub |
| Survey of India | copyright, licence by use (unverified) | licence agreement | ✗ | ✗ | no | n/a | Nakshe portal |
| USGS GNIS | public domain (✓) | credit requested | ✓ | ✓ | yes | bi-monthly | TNM staged |
| Census TIGER / Gazetteer / ZCTA | US federal work (§105; not verified on Census page) | none | ✓ | ✓ | yes | annual, 2026 | shapefile / txt |
| US NAD | NAD access and usage licence (unverified) | unknown | ◐ | ◐ | unknown | periodic | DOT |

## 5. What we build ourselves, and why

1. **Lite time zones.** No open, non-ODbL timezone polygons exist. Build them from lite admin polygons (COD-AB, geoBoundaries BY / PD layers, TIGER, Natural Earth) mapped to tzdb zones using `zone1970.tab` and its country codes (public domain), with manual sub-national splits where a country has more than one zone (USA, India none, Nepal none). Measurement: agreement with timezone-boundary-builder on a population-weighted sample of points. The comparison may run only in a main-side QA job whose output is a pass or fail number, never fed back into lite geometry. The gate must enforce that isolation.
2. **Provenance gate.** An allow-list keyed on (source, layer or source-prefix, licence string, lineage flag). Rules:
   - geoBoundaries via the `boundaryLicense` API field;
   - Who's On First via `src:*` properties;
   - Overture via each row's `sources` field;
   - any licence string outside {PD, CC0, CC BY 2.x–4.0, CC BY 3.0 IGO, GODL, CDLA-P 2.0, Apache 2.0, Copernicus DEM, NASA CC0} fails lite.

   Measurement: a test corpus of known-bad inputs (OSM, the India ODbL layers, NPL ADM3, Kontur) must all be rejected.
3. **Attribution generator.** It emits `ATTRIBUTION.md` per pack from the provenance records, including the GODL §5 statements and the Copernicus notice and disclaimer.

## 6. Impact on the plan

- **New ADR:** lite input allow-list and lineage policy (Kontur, WorldPop, GHSL, HRSL, and OSM-imported Wikidata claims).
- **New ADR:** the main build's ODbL compliance model. Packs are Derivative Databases; §4.6 is met by publishing the full pack free of charge. Search results are insubstantial extracts and SDK results carry attribution metadata.
- **Architecture:** provenance fields at feature and property level, not only per dataset. `ATTRIBUTION.md` and `LICENCE` ship inside every signed pack.
- **Roadmap, Nepal:** lite admin uses COD-AB adm1–3 rather than geoBoundaries ADM3. Wards (admin 4) are main-only from OSM until an open ward source is found.
- **Roadmap, India:** lite admin below the state level is blocked until pincode boundaries (GODL) or LGD terms are confirmed. All geoBoundaries layers below ADM1 are ODbL.
- **Gate:** "licence verified on primary page" becomes a release gate for every source in a pack.

## 7. Risks and open items

**ODbL and distribution (main):**

- Per-country main packs are Substantial extracts and Derivative Databases. They must be offered under ODbL, carry ODbL notices, and meet §4.6 (the pack itself, free online, satisfies it).
- Apps that embed a main pack "Publicly Convey" a Derivative Database, so the SDK must make the ODbL notice and attribution easy to surface. This is a documentation and API requirement, not optional.
- Signing is not an Effective Technological Measure, because the data stays readable. If packs are ever encrypted, ODbL §4.7 (a parallel unrestricted copy) and CC BY 4.0 §2(a)(5)(B) apply.

**Search results and MCP answers:**

- Under the 2017 geocoding guideline, individual results are insubstantial extracts, **not** Produced Works. The guideline rejects the Produced Work framing. They can be combined with third-party data without share-alike. Our geocoder and the apps using it must credit OSM.
- Risks: an MCP server answering bulk or systematic queries enables reconstruction of a substantial part (§6.2 repeated extraction). Mitigations: attribution in every answer's metadata, rate limits, no bulk "list all features in area" tools for main. An LLM answer quoting results arguably is a Produced Work (text); either way it needs an OSM credit.
- Open item for counsel: whether a vector-search index or embeddings over main data are a Derivative Database.

**Lite contamination:**

- Third-party "CC BY" products built with OSM (Kontur explicitly; WorldPop, GHSL and HRSL possibly) and Wikidata claims imported from OSM. Decision needed on whether lineage disqualifies. Recommendation: Kontur out. Check the WorldPop R2025A, GHSL R2023A and HRSL methodology papers for OSM inputs before admitting them.
- Overture places and addresses rows may carry ODbL-derived admin fields (unverified).

**Licence-specific:**

- **Copernicus:** the pass-through obligations (Art. 6(e)) inside a CC BY 4.0 lite pack need counsel review. Fallback: NASADEM (CC0) for lite.
- **GODL:** the provider can revoke a dataset (§7(c)). Packs need a takedown path. Arbitration and Indian law apply.
- **CC BY 3.0 IGO:** arbitration clause; no SA. Low risk.
- **CDLA-P 2.0** (Microsoft): Overture's ODbL label conflicts with Microsoft's own licence. Use Microsoft's.

**India legal (non-licence):** publishing maps that depict India's external boundary differently from the Survey of India depiction carries legal exposure in India. Boundary depiction policy for disputed areas needs its own note.

**Unverified items to close:** AW3D30 current terms; FABDEM primary page; SALB full terms PDF; Survey of India under NGP 2022; LGD terms; Census 2011 PCA on data.gov.in; the India pincode boundary licence page; US NAD licence; Census TIGER public-domain statement; sizes for OSM, Overture, Copernicus and Wikidata; CDLA-P 2.0 text re-read; the CC FAQ on adapter's licences.

**Drift:** the geoBoundaries per-layer licences, Overture source lists and Microsoft licence have all changed before. Re-run the matrix each quarter and on each Overture schema-change month.

### 7a. Draft `ATTRIBUTION.md` (main build)

```markdown
# Data attribution: teigeodata main build

This database is made available under the Open Database License (ODbL) 1.0:
https://opendatacommons.org/licenses/odbl/1-0/ . Any rights in individual
contents of the database are licensed under the Database Contents License where
applicable. You may obtain the complete database free of charge from <pack URL>.

Contains data © OpenStreetMap contributors, available under the Open Database
License (ODbL). https://www.openstreetmap.org/copyright

Contains data from Overture Maps Foundation, overturemaps.org
(© OpenStreetMap contributors, Overture Maps Foundation), release 2026-10-21.0.
Places source notices: Copyright 2024 Foursquare Labs, Inc. All rights reserved.
Other places sources under CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/).

Time zone boundaries from timezone-boundary-builder 2026f (ODbL),
https://github.com/evansiroky/timezone-boundary-builder. Time zone rules from the
IANA Time Zone Database 2026f (public domain).

Data from Who's On First. License: https://whosonfirst.org/docs/licenses/

Contains data from GeoNames (https://www.geonames.org/), CC BY 4.0.

Boundaries from geoBoundaries (https://www.geoboundaries.org), Runfola et al.
(2020) PLoS ONE 15(4): e0231866, with per-layer licences as listed in
sources.json (including CC BY 4.0, CC BY 3.0 IGO, CC BY 2.5 IN, ODbL 1.0,
public domain).

Administrative boundaries and population (COD-AB / COD-PS) from OCHA via the
Humanitarian Data Exchange, CC BY 3.0 IGO: Nepal: Survey Department of Nepal /
UN RCO Nepal; Bangladesh: BBS; Sri Lanka: Survey Department of Sri Lanka;
Pakistan: WFP SDI. Modified by teispace.

Made with Natural Earth (public domain).

Wikidata (CC0).

produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © Airbus Defence
and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and
ESA; all rights reserved.
produced using Copernicus WorldDEM™-90 © DLR e.V. 2010-2014 and © Airbus Defence
and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and
ESA; all rights reserved.
The organisations in charge of the Copernicus programme by law or by delegation
do not incur any liability for any use of the Copernicus WorldDEM-30.
The organisations in charge of the Copernicus programme by law or by delegation
do not incur any liability for any use of the Copernicus WorldDEM™-90.

NASADEM / SRTM: NASA, distributed by NASA LP DAAC. ICESat-2 ATL08:
Neuenschwander et al. (2023), NSIDC DAAC, doi:10.5067/ATLAS/ATL08.006.

U.S. Geological Survey: GNIS and 3D Elevation Program (public domain).
Credit: U.S. Geological Survey.
U.S. Census Bureau: TIGER/Line Shapefiles, Gazetteer Files and ZCTAs, 2026.

Google Open Buildings v3 (CC BY 4.0 / ODbL), Sirko et al. (2021) arXiv:2107.12283.
Microsoft Global ML Building Footprints, CDLA-Permissive-2.0
(agreement text: https://cdla.dev/permissive-2-0/).

© ESA WorldCover project 2021 / Contains modified Copernicus Sentinel data (2021)
processed by ESA WorldCover consortium. Zanaga et al. (2022) doi:10.5281/zenodo.7254221.

GHSL: European Commission, Joint Research Centre, GHS-POP / GHS-BUILT R2023A.
WorldPop (www.worldpop.org), R2025A Global2, CC BY 4.0.
High Resolution Population Density Maps + Demographic Estimates by CIESIN and
Meta (CC BY 4.0); imagery © 2016 Maxar.
Kontur Population dataset (CC BY), Kontur, 2023-11-01.

India: Department of Posts, <year>, All India Pincode Directory, Open Government
Data Platform India, <dd/mm>, https://www.data.gov.in/resource/all-india-pincode-directory-till-last-month.
Published under Government Open Data License - India:
https://www.data.gov.in/Godl . (One GODL §5 statement per dataset used.)
India boundaries by DataMeet India community (CC BY 4.0).

No data provider endorses teigeodata or its users.
```

### 7b. Per country: what lite can realistically include

| Component | Nepal | India | USA |
|---|---|---|---|
| Admin hierarchy and polygons | COD-AB adm1–3 (7/77/775, CC BY-IGO); geoBoundaries ADM0 (CC BY) and ADM1 (IGO). **No wards** | geoBoundaries ADM1 (CC BY 2.5 IN, 2011, stale); DataMeet (CC BY 4.0, check lineage). **Districts and below blocked** until pincode boundaries (GODL) or LGD terms are confirmed | TIGER/Line 2026 (states, counties, places, county subdivisions) |
| Admin codes | COD p-codes | LGD codes (licence unverified); Census 2011 codes via GODL copies (unverified) | FIPS / GEOIDs (TIGER) |
| Places and names | GeoNames, Wikidata, WOF (filtered, after audit) | GeoNames, Wikidata, pincode directory post offices (GODL) | GNIS, Census Gazetteer, GeoNames, Wikidata |
| Postal codes | **none** (GeoNames has no NP; Nepal Post terms unverified) | pincode directory with lat/lon (GODL, monthly); boundaries if confirmed | ZCTA 2026 (Census); GeoNames US |
| Former postal codes | none | diff monthly pincode snapshots ourselves | diff ZCTA vintages; GeoNames |
| Elevation | NASADEM (CC0); Copernicus GLO-30 with notices, pending counsel | same | 3DEP (public domain) |
| Population | COD-PS adm0–2 (CC BY-IGO); WorldPop / GHSL if lineage is cleared | WorldPop / GHSL if cleared; Census 2011 PCA via GODL (unverified) | Census (public domain, decennial / ACS; terms not checked today) |
| Timezone | own build from tzdb (single zone, Asia/Kathmandu) | own build (single zone, Asia/Kolkata) | own build from admin polygons + tzdb (multi-zone, hardest) |
| Buildings (for centroids and density) | Google v3 (CC BY), Microsoft (CDLA-P) | Google v3, Microsoft | Microsoft |

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Overture Maps | 2026-10-21.0, schema v2.0.0 | 2026-11-06 | docs.overturemaps.org/release-calendar |
| Overture attribution page | updated 2026-05-15 | 2026-11-06 | docs.overturemaps.org/attribution |
| OSMF Geocoding guideline | endorsed 2017-08-24 | 2026-11-06 | osmfoundation.org |
| OSMF Substantial guideline | endorsed 2014-06-06 | 2026-11-06 | osmfoundation.org |
| OSMF Collective DB guideline | endorsed 2016-06-17 | 2026-11-06 | osmfoundation.org |
| OSMF Attribution guidelines | adopted 2021-06-25 | 2026-11-06 | osmfoundation.org |
| timezone-boundary-builder | 2026f (2026-11-01) | 2026-11-06 | GitHub API |
| IANA tzdb | 2026f (2026-10-27) | 2026-11-06 | iana.org |
| GeoNames dumps | regenerated 2026-11-06 | 2026-11-06 | download.geonames.org |
| geoBoundaries | 6.0.0; layers built 2023-12-12 | 2026-11-06 | geoboundaries.org API |
| HDX COD-AB NPL | v02, modified 2026-08-14 | 2026-11-06 | HDX CKAN API |
| Copernicus DEM | 2024_1 | 2026-11-06 | dataspace.copernicus.eu |
| ATL08 | v6 retired, v7 current | 2026-11-06 | nsidc.org |
| AW3D30 | v4.1 (2024), terms unread | 2026-11-06 | eorc.jaxa.jp (search excerpt) |
| Google Open Buildings | v3 | 2026-11-06 | sites.research.google |
| Microsoft ML Buildings | refresh 2026-08-13 | 2026-11-06 | GitHub README |
| WorldPop | R2025A v1 Global2 | 2026-11-06 | hub.worldpop.org |
| GHSL | R2023A | 2026-11-06 | JRC (partly via search) |
| Kontur Population | 2023-11-01 | 2026-11-06 | HDX CKAN API |
| ESA WorldCover | 2020 v100, 2021 v200 | 2026-11-06 | esa-worldcover.org |
| SHRUG | 2.2 (August 2026) | 2026-11-06 | devdatalab.org |
| TIGER/Line, Gazetteer | 2026 (TIGER released 2026-10-14) | 2026-11-06 | census.gov |
| India pincode directory | data up to 2026-05-24 | 2026-11-06 | data.gov.in page metadata |
| GODL | Gazette 2017-02-13 | 2026-11-06 | data.gov.in PDF |

## 9. Sources

- [OpenStreetMap copyright and licence](https://www.openstreetmap.org/copyright), OSMF, accessed 2026-11-06.
- [ODbL 1.0 legal text](https://opendatacommons.org/licenses/odbl/1-0/), Open Data Commons, accessed 2026-11-06.
- [Geocoding guideline](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Geocoding_-_Guideline), OSMF, 2017, accessed 2026-11-06.
- [Substantial guideline](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Substantial_-_Guideline), OSMF, 2014, accessed 2026-11-06.
- [Collective Database guideline](https://osmfoundation.org/wiki/Licence/Community_Guidelines/Collective_Database_Guideline_Guideline), OSMF, 2016, accessed 2026-11-06.
- [Attribution guidelines](https://osmfoundation.org/wiki/Licence/Attribution_Guidelines), OSMF, 2021, accessed 2026-11-06.
- [Overture attribution and licensing](https://docs.overturemaps.org/attribution/), Overture Maps Foundation, updated 2026-05-15, accessed 2026-11-06.
- [Overture release calendar](https://docs.overturemaps.org/release-calendar/), accessed 2026-11-06.
- [Who's On First licences](https://whosonfirst.org/docs/licenses/), [data LICENSE](https://github.com/whosonfirst-data/whosonfirst-data/blob/master/LICENSE.md), [sources list](https://github.com/whosonfirst/whosonfirst-sources/blob/main/sources/README.md), [downloads](https://whosonfirst.org/download/), accessed 2026-11-06.
- [Wikidata:Licensing](https://www.wikidata.org/wiki/Wikidata:Licensing) and [Wikipedia:Copyrights](https://en.wikipedia.org/wiki/Wikipedia:Copyrights), Wikimedia, accessed 2026-11-06.
- [GeoNames about](https://www.geonames.org/about.html) and [postal readme](https://download.geonames.org/export/zip/readme.txt), accessed 2026-11-06.
- [geoBoundaries](https://www.geoboundaries.org/), [API NPL](https://www.geoboundaries.org/api/current/gbOpen/NPL/ALL/), [API IND](https://www.geoboundaries.org/api/current/gbOpen/IND/ALL/), [API USA ADM2](https://www.geoboundaries.org/api/current/gbOpen/USA/ADM2/), [GitHub README](https://github.com/wmgeolab/geoBoundaries), accessed 2026-11-06.
- [UN SALB Terms of Use 2021](https://salb.un.org/sites/default/files/wysiwyg_uploads/docs_uploads/TermsOfUseSALB2021.pdf), UN, search excerpts only, accessed 2026-11-06.
- [GADM licence](https://gadm.org/license.html), accessed 2026-11-06.
- [Natural Earth terms of use](https://www.naturalearthdata.com/about/terms-of-use/), accessed 2026-11-06.
- HDX CKAN API, `package_show` for [cod-ab-npl](https://data.humdata.org/dataset/cod-ab-npl), cod-ab-bgd, cod-ab-lka, cod-ab-pak, cod-ps-npl and [kontur-population-dataset](https://data.humdata.org/dataset/kontur-population-dataset), OCHA, accessed 2026-11-06.
- [CC BY 3.0 IGO legal code](https://creativecommons.org/licenses/by/3.0/igo/legalcode) and [CC FAQ](https://creativecommons.org/faq/), Creative Commons, accessed 2026-11-06.
- [Copernicus DEM licences (PDF)](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf) and [COP-DEM collection page](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM), ESA / EU, accessed 2026-11-06.
- [Copernicus DEM on AWS](https://registry.opendata.aws/copernicus-dem/), accessed 2026-11-06.
- [NASA Earthdata data use guidance](https://www.earthdata.nasa.gov/engage/open-data-services-software-policies/data-use-guidance), NASA, accessed 2026-11-06.
- [ATL08 v6](https://nsidc.org/data/atl08/versions/6), NSIDC, accessed 2026-11-06.
- [JAXA AW3D30](https://www.eorc.jaxa.jp/ALOS/en/dataset/aw3d30/) and [terms](https://www.eorc.jaxa.jp/ALOS/en/aw3d30/terms.htm), JAXA, terms text not rendered, accessed 2026-11-06.
- FABDEM: [Fathom](https://www.fathom.global/academic-licenses/) and secondary catalogues; [Bristol data page](https://data.bris.ac.uk/data/dataset/s5hqmjcdj8yo2ibzi9b4ew3sn) timed out, accessed 2026-11-06.
- [USGS 3DEP products](https://www.usgs.gov/3d-elevation-program/about-3dep-products-services), [USGS copyrights and credits](https://www.usgs.gov/information-policies-and-instructions/copyrights-and-credits), [BGN domestic names](https://www.usgs.gov/us-board-on-geographic-names/domestic-names), USGS, accessed 2026-11-06.
- [Google Open Buildings](https://sites.research.google/gr/open-buildings/), Google Research, accessed 2026-11-06.
- [Microsoft Global ML Building Footprints](https://github.com/microsoft/GlobalMLBuildingFootprints), Microsoft, accessed 2026-11-06.
- [GHSL download page](https://human-settlement.emergency.copernicus.eu/download.php) and [how to cite](https://human-settlement.emergency.copernicus.eu/GHSLhowToCite.php), JRC, accessed 2026-11-06.
- [WorldPop hub](https://hub.worldpop.org/project/categories?id=3), accessed 2026-11-06.
- [Kontur Population](https://www.kontur.io/datasets/population-dataset/), Kontur, accessed 2026-11-06.
- [Meta HRSL on AWS](https://registry.opendata.aws/dataforgood-fb-hrsl/), accessed 2026-11-06.
- [timezone-boundary-builder](https://github.com/evansiroky/timezone-boundary-builder) README and releases, accessed 2026-11-06.
- [IANA time zones](https://www.iana.org/time-zones) and [tz LICENSE](https://github.com/eggert/tz/blob/main/LICENSE), accessed 2026-11-06.
- [ESA WorldCover data access](https://esa-worldcover.org/en/data-access), ESA, accessed 2026-11-06.
- [Government Open Data License – India, Gazette notification](https://www.data.gov.in/sites/default/files/Gazette_Notification_OGDL.pdf), MeitY, 2017-02-13, accessed 2026-11-06.
- [All India Pincode Directory](https://www.data.gov.in/resource/all-india-pincode-directory-till-last-month), Department of Posts / data.gov.in, accessed 2026-11-06.
- [Delivery Post Office Pincode Boundary](https://data.gov.in/resource/delivery-post-office-pincode-boundary) and [AIKosh listing](https://aikosh.indiaai.gov.in/home/datasets/details/all_india_pincode_boundary_geo_json.html), search excerpts only, accessed 2026-11-06.
- [DIGIPIN](https://github.com/INDIAPOST-gov/digipin), India Post, accessed 2026-11-06.
- [Local Government Directory](https://lgdirectory.gov.in/), MoPR / NIC, accessed 2026-11-06.
- [SHRUG download](https://www.devdatalab.org/shrug_download/), Development Data Lab, accessed 2026-11-06.
- [DataMeet maps](https://github.com/datameet/maps), DataMeet, accessed 2026-11-06.
- [Survey of India map policy guidelines](https://www.surveyofindia.gov.in/documents/guidelines-for-implementing-national-map-policy.pdf) and [CIS on Open Series Maps](https://cis-india.org/openness/survey-of-india-open-series-maps-fails-to-implement-public-sharing-of-govt-data), search excerpts, accessed 2026-11-06.
- [Census TIGER/Line](https://www.census.gov/geographies/mapping-files/time-series/geo/tiger-line-file.html) and [Gazetteer files](https://www.census.gov/geographies/reference-files/time-series/geo/gazetteer-files.html), US Census Bureau, accessed 2026-11-06.
