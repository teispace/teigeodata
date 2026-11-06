---
title: "Competitive analysis"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-06
---

# Competitive analysis

## 1. Question

Who else gives developers places, admin hierarchy, boundaries, reverse geocoding, timezones and elevation, and how good is each one in Nepal and India? Which offer it offline, under what licence, at what size, in which languages, and how often do they update? What can teigeodata own, what must it match before anyone takes it seriously, what could overtake it, and which numbers should it publish against which rival?

This note feeds:

- the positioning on the docs site and README;
- the benchmark harness and the "measured" pages ([quality bar](../../06-engineering/quality-bar.md));
- the ID-stability and update-model design for packs;
- the per-country release gates (Nepal first, then India).

It complements, and does not repeat, the licence detail in [source-licences.md](source-licences.md), the search prior art in [names-and-search.md](names-and-search.md), the Nepal inputs in [nepal-sources.md](nepal-sources.md) and the timezone and elevation libraries in [timezone-and-elevation.md](timezone-and-elevation.md).

Conventions: **verified** means read on the vendor's or project's own page, repository or registry on 2026-11-06. **Unverified** means it comes from a secondary source, a forum, memory, or a page that did not load, and must be checked before anyone quotes it in public. Prices are list prices on 2026-11-06 and change often.

## 2. Summary and recommendation

1. **Nobody ships what we plan to ship.** No product, open or commercial, combines (a) a deduplicated, cleaned gazetteer to the lowest admin level in Nepal and India, (b) local-script names with cross-script search, (c) exact IANA timezone and elevation with stated uncertainty, (d) offline packs of a few MB, signed and updated by delta, and (e) one API across a dozen languages. Each rival covers one or two of these.
2. **The open field splits three ways, and each part has a known flaw:**
   - *Datasets* (GeoNames, Who's On First, Overture, Wikidata, geoBoundaries, Natural Earth) are raw material, not products. GeoNames is the default for offline libraries and is the weakest locally. Overture divisions are, in Overture's own words, "spotty" below county level, and its places carry "duplicates, a high junk rate, and low property completeness".
   - *Server geocoders* (Nominatim, Photon, Pelias) are accurate where OSM is good, but heavy. A Nominatim planet needs at least 1 TB of disk and 128 GB of RAM is "strongly recommended". A Photon planet needs about 95 GB. None runs on a phone.
   - *Embedded libraries* (reverse_geocoder, local-reverse-geocoder, geocoder_offline, country-coder, tzf-rs, timezonefinder, geo-tz) are small and fast but do one thing each. Most reverse-geocode to the nearest GeoNames city, not to the containing admin polygon. Several are unmaintained.
3. **Commercial APIs forbid the use case.** Google lets you cache Geocoding and Places coordinates for at most 30 consecutive calendar days. HERE's platform terms cap storage at 30 days unless you buy a Permanent Geocoding licence, and even then you may not redistribute. Mapbox temporary results may not be cached at all, and permanent results ($5.00 per 1,000) are for "your own personal or business use". Esri charges 8x more for stored geocodes ($4.00 vs $0.50 per 1,000). Apple allows only temporary caching. None offers an offline gazetteer a developer may ship inside an app, except Mapbox's offline search, which is a premium, sales-only feature.
4. **Regional players are online-only and closed.** Baato (Nepal, by Kathmandu Living Labs) and Galli Maps (Nepal) are pay-as-you-go REST APIs. Mappls (MapmyIndia) and Ola Maps (India) price aggressively (Ola: 100,000 free calls per API per month, then ₹0.099 per geocode) but none publishes offline data, accuracy figures or an open licence. They are partners or benchmarks, not direct rivals for an offline SDK.
5. **Recommendation.** Position teigeodata as *"the open, offline gazetteer you are allowed to keep"*: accurate to the ward in Nepal and the village in India, small enough to bundle, with published error and recall numbers. Match the table stakes (prefix and fuzzy search, reverse to admin polygon, timezone, a hosted-API-shaped HTTP server, Nominatim-compatible output). Benchmark publicly against GeoNames, Overture divisions, Nominatim, Photon, Google Geocoding (for accuracy only, within its terms) and tzf-rs / timezonefinder.
6. **Biggest risk:** Overture fills in admin depth below county and its places de-duplication improves, and a well-funded team wraps it in an offline SDK. Our defence is depth and correctness in Nepal and India first, local-script search, published accuracy, and a portable pack format that could ingest Overture itself.
7. **Main things to copy:** Who's On First's immutable integer IDs with `supersedes` / `superseded_by`; Overture's monthly release train and GERS registry, changelog and bridge files; Photon's swap-directory updates; Nominatim's variant lexicon; OpenCage's "store results forever" promise; tzf-rs's precompiled, compact data. **Main things to avoid:** Carmen's dependency on one vendor, Gisgraphy's single-maintainer stall, reverse_geocoder's frozen data, WOF's monolithic Git repos, and Overture's one-time 100% ID churn.

## 3. Findings

### 3.1 Open gazetteers and datasets

**GeoNames** ([about](https://www.geonames.org/about.html), [export](https://www.geonames.org/export/)). Verified:
- Over 32 million names, over 13 million unique features, 5.2 million populated places, 19 million alternate names, 684 feature codes. Licence CC BY 4.0. Daily database export; wiki-style edits, with some records locked.
- Sources include NGA / US BGN, USGS GNIS, Ordnance Survey and GeoBase. Elevation from GTOPO30.
- Free web services: 10,000 credits per day and 1,000 per hour per username. Premium services have an SLA.
- Sizes on 2026-11-06 (HTTP `Content-Length`): `allCountries.zip` 422.5 MB, `cities500.zip` 13.9 MB, `IN.zip` 15.8 MB, `NP.zip` 3.5 MB. All regenerated 2026-11-06.
- Known problems: coordinates rounded to arc-minutes in older imports, duplicates, stale hierarchies. Nepal's 2017 federal structure is only partly reflected ([vision](../../00-vision/vision.md); Ahlers 2013 measured positional error in GeoNames). The postal dump has no Nepal file ([source-licences.md](source-licences.md)).
- Verdict: the de facto default for every offline library below, and the baseline we must beat in public.

**Who's On First (WOF)** ([whosonfirst.org](https://whosonfirst.org/), [Geocode Earth WOF data](https://geocode.earth/data/whosonfirst/)). Verified:
- About 26 million records, 21 million venues, 219 countries. Distributed as raw GeoJSON, bundles and SQLite; Geocode Earth hosts global combined and legacy per-placetype downloads.
- Stable integer IDs per place, with `supersedes` / `superseded_by` and a rich placetype ladder (continent to microhood). These two ideas are the best ID design in the field (ID semantics: from WOF docs, not re-read today, **unverified** detail).
- Licence: CC0 structure, per-source data licences, including non-commercial and ODbL sources ([source-licences.md](source-licences.md) §3.3).
- Activity: the umbrella `whosonfirst-data/whosonfirst-data` repo was last pushed 2024-03-06 with 646 open issues (GitHub API). The per-country admin repos were not checked (rate limit, **unverified**). Pelias still depends on WOF for admin lookup.
- Nepal and India depth: not measured today (**unverified**). WOF admin data for South Asia is largely imported from older Quattroshapes / GeoNames / Natural Earth lineages (**unverified**).

**Overture Maps** ([GERS](https://docs.overturemaps.org/gers/), [stability](https://docs.overturemaps.org/gers/stability/), [places](https://docs.overturemaps.org/guides/places/), [divisions](https://docs.overturemaps.org/guides/divisions/), [addresses](https://docs.overturemaps.org/guides/addresses/)). Verified:
- Latest release 2026-10-21.0; monthly releases; schema changes only in quarterly months ([source-licences.md](source-licences.md) §3.2).
- **Places:** about 88 million records (Meta about 60 M, BrightQuery about 16 M, Microsoft about 5.9 M, Foursquare about 3.8 M, plus PinMeTo, AllThePlaces and others). No OSM. Per-source CDLA-Permissive 2.0 / Apache-2.0 / CC0. The guide itself states the data contains "duplicates, a high junk rate, and low property completeness". `confidence` is existence-only and not calibrated across providers. `categories` was replaced by `taxonomy` and `basic_category` in the September 2026 release (a breaking change).
- **Divisions:** over 5.8 million features in three types (`division` point, `division_area`, `division_boundary`). Subtypes from country down to microhood. "Coverage below county is spotty." Built from OSM (primary) and geoBoundaries; theme licence **ODbL**. The guide says nothing specific about India or Nepal.
- **Addresses:** over 475 million points in 41 countries, **Alpha**. Neither India nor Nepal is included.
- **GERS:** 128-bit random UUIDs since release 2025-06-25.0, which replaced every earlier ID ("100% churn by design"; a one-off old-to-new mapping was published). The registry records first seen, last seen and last changed. A data changelog and bridge files (source ID to GERS ID) ship monthly. Stability rules: a building keeps its ID only while IoU > 0.5; a place or division can get a new ID on a name change if not otherwise matched; on a split only one piece keeps the ID; merges get new IDs; and **old-to-new traceability was "parked" in February 2025**, so the changelog shows `removed` and `added` without a link.
- Access: GeoParquet on S3 and Azure. Sizes not checked (**unverified**).

**OpenStreetMap-derived extracts.** Geofabrik country extracts and planet PBF weekly, minutely diffs; ODbL ([source-licences.md](source-licences.md) §3.1). Nepal has active local mapping (Kathmandu Living Labs, HOT) and ward boundaries are being mapped ([nepal-sources.md](nepal-sources.md)). India is uneven, and its external boundary depiction is contested. Raw OSM is not a gazetteer: places are nodes with `place=*`, admin units are relations of varying completeness, and duplicates between nodes and relations are common.

**Wikidata.** CC0 structured data, weekly dumps (size over 100 GB compressed, **unverified**). Strong on cross-IDs (GeoNames, OSM, WOF, census codes) and multilingual labels. Weak on coordinates for small Nepali settlements and on current admin membership (**unverified** for Nepal; spot-checks recommended).

**Natural Earth.** Public domain. Small, world-skeleton only (1:10m, 1:50m, 1:110m). The project blog's last full release note is 4.1.0 (2018-05-23); the GitHub vector repo was last pushed 2024-04-22 (GitHub API). A 5.1.x release exists on GitHub (**unverified** exact version and date). De facto boundary choices.

**geoBoundaries** ([geoboundaries.org](https://www.geoboundaries.org/)). About 1 million boundaries in over 200 entities, tracked since 2016. Site states CC BY 4.0, but per-layer licences differ: Nepal ADM3 CC BY-SA, India ADM2 to ADM5 ODbL ([source-licences.md](source-licences.md) §3.6). Repo last pushed 2026-11-01 with 154 open issues.

**GADM.** Non-commercial, no redistribution without permission ([licence](https://gadm.org/license.html), verified in [source-licences.md](source-licences.md)). Version 4.1 (2022) is the current one (**unverified** today; the home page did not state it). Widely used in research for India sub-districts and Nepal; unusable for us.

**OpenAddresses.** BSD-3-Clause code, per-source data licences; repo last pushed 2026-11-05 with 1,028 open issues. No `sources/in` or `sources/np` directory exists (both return 404 on GitHub, 2026-11-06). No address data for our first two countries.

### 3.2 Open geocoders (server)

| Project | Licence | Activity (GitHub API, 2026-11-06) | Footprint | Notes |
|---|---|---|---|---|
| **Nominatim** | GPL-3.0 | 5.3.3 released 2026-10-14; 4,553 stars; 104 open issues | Planet: ≥ 1 TB disk, 128 GB RAM "strongly recommended", 2.5 days import on NVMe, 4–5 days on SSD; min 2 GB RAM ([install docs](https://nominatim.org/release-docs/latest/admin/Installation/)) | PostgreSQL/PostGIS. Best open ranking (Wikipedia importance) and the ICU variant lexicon ([names-and-search.md](names-and-search.md)). Public instance is rate-limited by the OSMF usage policy (1 req/s, **not re-read today**). |
| **Photon** | Apache-2.0 | Last push 2026-11-02; 3,176 stars; 42 open issues | Planet ≈ 95 GB disk, growing ~10% a year; ≥ 64 GB RAM recommended; Java 21 ([README](https://github.com/komoot/photon)) | Search-as-you-type, typo tolerance, location bias, reverse. Dumps carry en/de/fr plus local language only. Updates by unpacking into a new directory and swapping. OpenSearch 3.x or embedded. |
| **Pelias** | MIT | Meta repo last push 2026-07-24; `api` last push 2026-10-21; 3,600 stars; 234 open issues on meta repo | Elasticsearch 7/8; planet RAM/disk not stated on README (**unverified**, community figures run to tens of GB) | Maintained by Geocode Earth (two core maintainers). Modular: WOF admin, PIP service, Placeholder, libpostal, interpolation. Importers for OSM, OpenAddresses, WOF, GeoNames, CSV. |
| **Carmen** (Mapbox) | BSD-2-Clause | `github.com/mapbox/carmen` returns **404** (removed or made private); last npm publish `@mapbox/carmen` 34.1.4 on 2022-02-16 | Vector-tile index | Effectively dead as open source. A fork (`headwaymaps/carmen`) exists. |
| **libpostal** | MIT | Last push 2026-05-13; 4,919 stars; 300 open issues | Default model ≈ 1.8 GB on disk; Senzing alternative ≈ 2.2 GB ([Senzing](https://senzing.com/what-is-libpostal/), secondary) | Address parsing and normalisation, not geocoding. No India-specific accuracy published; a related study (deepparse) reports India among the weaker countries (secondary). Too large for mobile. |
| **Gisgraphy** | LGPL-3.0 | Last push 2023-12-27; 391 stars | Java, PostgreSQL, Solr | Stalled; single maintainer. Also sells a hosted API. |
| **OpenCage** (commercial wrapper) | Proprietary service | Active ([pricing](https://opencagedata.com/pricing)) | Hosted only | Free trial 2,500 req/day at 1 req/s. Plans €45/month (10,000/day) to €900/month (300,000/day), Enterprise from €1,800. **Results may be stored permanently, "even if you are no longer a customer".** Built on OSM and other open sources. |

Accuracy in Nepal: Nominatim resolves Nepali places to OSM features where they exist, e.g. "Lahachowk" to Machhapuchchhre-04, Kaski ([names-and-search.md](names-and-search.md)). A Nepali academic study matched 267 addresses: OSM returned 118 results, Google 86, and Google "gave huge mismatches for some cases" ([NJG, nepjol.info](https://nepjol.info/index.php/NJG/article/view/23004/19535), date not confirmed, **unverified** detail). Devanagari search depends on OSM `name:ne` coverage; Photon dumps do not carry `ne` names beyond the local `name` (inference from the README language list).

### 3.3 Offline and embedded libraries

| Library | Ecosystem | Latest (registry, 2026-11-06) | Data | What it does | Notes |
|---|---|---|---|---|---|
| `reverse_geocoder` (thampiman) | PyPI | 1.5.1, 2016-09-15; repo last push 2023-03-19; 1,910 stars | GeoNames cities with pop > 1000 | Nearest city by k-d tree | LGPL. Returns the nearest city, not the containing admin unit. Data frozen at build. |
| `reverse_geocode` | PyPI | 1.6.6, 2025-05-15 | GeoNames | Nearest city / country | LGPL. |
| `local-reverse-geocoder` | npm | 0.16.8, 2026-06-14 (Apache-2.0) | Downloads GeoNames at runtime | Nearest cities with admin1/admin2 | Actively maintained by one person. |
| `offline-geocoder` | npm | 1.0.0, 2018-08-02 | GeoNames in SQLite | Nearest city | Abandoned. |
| `geocoder_offline` | pub.dev | 0.2.1, 2021-08-11 | User-supplied CSV | Nearest point | Abandoned. Other pub.dev results (`geonames_offline`, `bd_offline_geocoder`, `thai_provinces_geo`) are single-country or single-purpose. |
| `geocoding` (Flutter plugin) | pub.dev | 5.0.1, 2026-10-28 | Wraps platform geocoders | Forward/reverse | Online: uses Android `Geocoder` and iOS `CLGeocoder`. |
| `reverse_geocoder` | crates.io | 4.1.1, 2024-03-05 | GeoNames cities | Nearest city | 150 k downloads. `rgeo` 0.3.0 (2022) similar. |
| `country-boundaries` | crates.io | 1.2.0, 2023-06-09 | OSM-derived country/subdivision raster | Point to ISO 3166 | Fast, tiny; country and some subdivisions only. |
| `@rapideditor/country-coder` | npm | 5.6.1, 2026-01-31 (ISC) | Hand-built borders | Point to country/region codes, plus metadata | Maintained by the RapiD/iD editors. Country-level only. |
| `which-polygon` (Mapbox) | npm | 2.2.1, 2023-02-16 (ISC) | Any GeoJSON | Point-in-polygon index | Generic building block. |
| `tzf-rs` | crates.io | 2.3.0, 2026-11-02; 688 k downloads | timezone-boundary-builder (precompiled) | Point to IANA zone | MIT code, ODbL data. Bindings in Go, Python (`tzfpy`), JS/WASM, Swift (**unverified** full list). |
| `timezonefinder` | PyPI | 9.2.0, 2026-10-22 | timezone-boundary-builder | Point to IANA zone | MIT; data package `timezonefinder-data` 4.2026.6. |
| `geo-tz` | npm | 8.1.11, 2026-11-02 | timezone-boundary-builder | Point to IANA zone | MIT; Node only, loads data lazily. |
| `@photostructure/tz-lookup` | npm | 11.8.0, 2026-11-05 (CC0) | Simplified raster | Point to zone, approximate near borders | Small, fast, less exact. |
| Android GeoTZ (AOSP) | Android platform | `main`, S2 level 12 | timezone-boundary-builder | Device timezone from location | Inside the OS, not an app API ([timezone-and-elevation.md](timezone-and-elevation.md)). |

**Mobile platform geocoders.**
- Android `Geocoder` "requires a backend service that is not included in the core android framework"; apps must call `isPresent()`, and "lack of network connectivity may still cause these methods to return null or empty lists" ([Android reference](https://developer.android.com/reference/android/location/Geocoder)). It is online in practice.
- iOS `CLGeocoder` is free, online, and rate-limited by undocumented limits. An Apple engineer: "Requests are free, and there's not a way to pay for additional requests" ([Apple forums](https://developer.apple.com/forums/thread/736348)). Developers report about 50 requests per 60 s (forum observation, **unverified**).

**Elevation.**
- Open-Elevation: GPL-2.0, last push 2024-04-17, 48 open issues; self-host with SRTM. Public instance reliability is poor according to user reports (**unverified**).
- OpenTopoData ([site](https://www.opentopodata.org/)): MIT code, last push 2026-02-16. Public API: max 100 locations per request, 1 call/s, 1,000 calls/day. Datasets include SRTM 30/90 m, ASTER 30 m, EU-DEM 25 m. Self-host by Docker. No Copernicus GLO-30 on the public list.
- Neither offers a mobile offline option. Neither gives an uncertainty per point.

### 3.4 Commercial APIs

**Google Maps Platform** ([pricing](https://developers.google.com/maps/billing-and-pricing/pricing), verified 2026-11-06):

| SKU | Free per month | Price per 1,000 (0–100 K) | At 5 M+ |
|---|---|---|---|
| Geocoding (Essentials) | 10,000 | $5.00 | $0.38 |
| Autocomplete requests (Essentials) | 10,000 | $2.83 | $0.21 |
| Place Details Essentials / Pro | 10,000 / 5,000 | $5.00 / $17.00 | $0.38 / $1.28 |
| Text Search Pro / Enterprise | 5,000 / 1,000 | $32.00 / $35.00 | $2.40 / $2.63 |
| Time Zone (Essentials) | 10,000 | $5.00 | $0.38 |
| Elevation (Pro) | 5,000 | $5.00 | $0.38 |

- India has separate, lower pricing since 1 August 2024, when Google cut India Maps API prices by up to 70% after Ola's launch ([Google India pricing](https://developers.google.com/maps/billing-and-pricing/pricing-india); [Inc42](https://inc42.com/buzz/map-wars-google-offers-free-access-of-up-to-6800-to-indian-developers), 2024). Current India figures were not re-read (**unverified**).
- **Storage:** lat/lng from Geocoding, Places and others may be cached "for up to 30 consecutive calendar days, after which Customer must delete" them; place IDs may be stored indefinitely ([Service Specific Terms](https://cloud.google.com/maps-platform/terms/maps-service-terms), wording confirmed in dated versions 2020 to 2024; the current page did not render, **partly verified**). Google Maps content may not be shown on a non-Google map (**unverified** current wording).
- Accuracy in Nepal: complaints that Kathmandu street names and locations are invented or wrong ([Annapurna Express](https://theannapurnaexpress.com/story/14487/); [Gadgetbyte Nepal](https://www.gadgetbytenepal.com/?p=154220)); a 2019 ministerial complaint about map errors from dependence on Google ([Himalayan Times](https://thehimalayantimes.com/nepal/dependence-on-google-caused-errors-aryal/)). Mostly opinion and older reports, **indicative only**.

**Mapbox** ([pricing](https://www.mapbox.com/pricing), [Geocoding API](https://docs.mapbox.com/api/search/geocoding/), [offline search](https://docs.mapbox.com/android/search/guides/search-engine/offline)), verified:
- Temporary geocoding: 100,000 free per month, then $0.75 to $0.45 per 1,000. "Temporary results are not allowed to be cached."
- Permanent geocoding: $5.00 per 1,000 (to 500 k), $4.00 above; no free tier; "only available for your own personal or business use" (no redistribution).
- Search Box: sessions (50 suggests + 1 retrieve); standard 2,500 free, then $11.50 per 1,000.
- Offline: Maps SDK tile regions are capped at 750 tile packs cumulative. Offline Search for Android is a "premium feature" that needs a sales contact; a README called it private beta (**unverified** current status). iOS offline search not confirmed.
- India: `worldview=in` exists; Hindi has "limited" coverage. Nothing Nepal-specific.

**HERE.** Storage beyond 30 days is restricted under the HERE Platform Terms; a paid Permanent Geocoding add-on allows longer internal storage, but no resale, redistribution or third-party serving ([HERE KB](https://docs.here.com/here-kb/docs/permanent-geocoding-overview-licensing-and-usage-rules), verified via search excerpt). Free tier on the Base plan is reported as 30,000 or 250,000 transactions per month by different secondary sources, and the official plans page failed to load (**unverified**). HERE SDK has offline maps for navigation customers (**unverified** search scope).

**TomTom** ([pricing](https://docs.tomtom.com/pricing), verified): Geocoding and Reverse Geocoding 20,000 free requests per month; Places Search Suggest 10,000, Details 5,000, Discover 5,000 free per month; overage prices not shown on the page. Storage rules not on the pricing page (**unverified**).

**Esri ArcGIS Location Platform** ([pricing](https://location.arcgis.com/pricing/), verified): geocodes not stored 20,000 free per month then $0.50 per 1,000; **stored geocodes $4.00 per 1,000** (no free tier); batch stored $4.00; Places nearby search 500 free then $8.00 per 1,000; elevation 50,000 points free then $1.00 per 1,000. Stored results require the `forStorage=true` flag.

**Apple MapKit.** MapKit JS and Maps Server API share a quota of 25,000 service calls per day per team (forum reports; Apple dashboard shows some limits per month, **unverified**). Caching is limited to temporary and "only as necessary" under the licence schedule (forum quote of Schedule 6 §2.5, **unverified**). Apple Maps coverage in Nepal is thin (**unverified**).

**what3words** ([pricing](https://what3words.com/pricing)): proprietary grid; free tier for small use, Basic £7.99/month per support pages (**partly verified**). The algorithm and word list are closed; a 2021 legal threat against an open re-implementation (WhatFreeWords) is widely reported (**unverified** here). Not a gazetteer.

**DIGIPIN** ([INDIAPOST-gov/digipin](https://github.com/INDIAPOST-gov/digipin), verified): India Post and IIT Hyderabad; about 4 m × 4 m cells; 10-character code (display 3-4-3); repository README states **Apache-2.0** (this supersedes the NOASSERTION reading in [source-licences.md](source-licences.md)). Reference implementation is a Node.js / Express API. Mappls offers a DIGIPIN conversion API and integrated it in its app in July 2025 ([MediaNama](https://www.medianama.com/2025/07/223-mapmyindias-mapplspin-india-posts-digipin/)). An open, offline encoder/decoder is cheap for us to offer.

### 3.5 Regional players

**Baato (Nepal)** ([pricing](https://baato.io/pricing), verified): by Kathmandu Living Labs (App Store listing). Free 5,000 credits per month, no expiry; Business usage-based, invoiced in NPR; Priority by contract. Business 25 req/s sustained, 50,000 requests/day cap; Priority 75 req/s, 100,000/day. APIs: search, reverse, nearby, places, routing, maps. Data sources are not named on the pricing page; Baato is OSM-based by origin (**unverified** current). Offline navigation in the consumer app is reported (secondary). No downloadable data.

**Galli Maps (Nepal).** Consumer app and APIs from Airlift Technologies' team; won the Startup World Cup Nepal Regional 2023 and the NYEF Startup Award; integrated into the e-Palika app ([b360nepal](https://www.b360nepal.com/galli-maps-wins-startup-world-cup-nepal-regional-2023); [ICT Frame](https://ictframe.com/digital-e-palika-and-galli-maps-collaboration)). API docs exist at gallimaps.com; developers on the Traccar forum call them "very poor" and report the reverse endpoint is not Nominatim-compatible ([Traccar forum](https://www.traccar.org/forums/topic/map-api/), about 2024). Pricing not public (**unverified**). A community Flutter package (`galli_map` 1.2.7) wraps autocomplete and reverse.

A 2023 Kathmandu Post report says Nepali apps "gain credence for their greater accuracy", based on developers' claims, not an independent test ([Kathmandu Post](https://kathmandupost.com/money/2023/03/06/nepali-made-location-apps-gain-credence-for-their-greater-accuracy)).

**Mappls / MapmyIndia (India)** ([about.mappls.com/api](https://about.mappls.com/api/), verified): "120+ APIs", "10+ Search APIs", "trusted by over 200,000+ developers", global APIs for "238 nations". No public prices (contact sales). Offline: NaviMaps offline navigation for embedded/automotive, not a developer gazetteer. eLoc (proprietary 6-character address code) and DIGIPIN conversion. Deepest commercial India coverage by reputation (villages, addresses), but no published accuracy figures (**unverified** claims).

**Ola Maps / Krutrim (India)** ([pricing](https://maps.olakrutrim.com/pricing), verified): 100,000 free calls per API per month; then Geocoding and Reverse ₹0.099 per request (100 K–5 M), ₹0.025 above; Autocomplete ₹0.056 / ₹0.013; Text Search ₹0.638 / ₹0.159; "Data stays in India". The 2024 launch promised 5 million free calls per month and 3 years free for ONDC startups (press, 2024); the free tier has since shrunk. A Fair Usage Policy took effect 1 September 2026 ([PDF](https://maps.olakrutrim.com/legal-docs/fair-usage-policy-2026.pdf), not read in full). Storage terms not on the pricing page (**unverified**).

### 3.6 Storage terms in one place

| Provider | May you keep results? | Source |
|---|---|---|
| Google | Lat/lng ≤ 30 days; place IDs forever | Service Specific Terms (dated versions 2020–2024) |
| Mapbox | Temporary: no caching. Permanent: yes, own use only, $5/1,000 | Geocoding API docs, pricing |
| HERE | ≤ 30 days; Permanent add-on for internal use, no redistribution | HERE KB |
| Esri | Only with `forStorage=true`, $4/1,000 | ArcGIS pricing |
| Apple | Temporary only | Forum quote of licence (**unverified**) |
| TomTom, Mappls, Ola, Baato, Galli | Not stated on pricing pages (**unverified**) | — |
| OpenCage | Yes, forever, even after cancelling | OpenCage pricing |
| Nominatim / Photon / Pelias self-hosted | Yes, under ODbL for OSM-derived data | OSMF geocoding guideline ([source-licences.md](source-licences.md)) |
| teigeodata | Yes: the data *is* the product (ODbL main, CC BY lite) | [vision](../../00-vision/vision.md) |

## 4. Options compared

### 4.1 Comparison matrix

Depth: ●●● to ward / village, ●● to district / sub-district, ● to province / state, ○ none. "Offline" means a developer may ship it inside an app and query with no network. Ratings for Nepal and India depth are this note's judgement from the cited docs, not measurements (**unverified** until the benchmark runs).

| Option | Nepal depth | India depth | Licence | Size | Offline | Bindings | DX | Update cadence | Community |
|---|---|---|---|---|---|---|---|---|---|
| GeoNames dump | ●● (stale) | ●● | CC BY 4.0 | NP 3.5 MB, IN 15.8 MB zip | Yes | Any (TSV) | Raw TSV | Daily | Large, old |
| Who's On First | ● to ●● (**unverified**) | ●● (**unverified**) | CC0 + per source | Large Git / SQLite | Yes | Any (GeoJSON/SQLite) | Hard | Irregular | Small |
| Overture divisions | ●● ("spotty" below county) | ●● | ODbL | GeoParquet, large | Yes, heavy | Any (Parquet) | SQL / DuckDB | Monthly | Corporate, growing |
| Overture places | POIs only | POIs only | Per source | Large | Yes, heavy | Any | SQL | Monthly | Corporate |
| OSM extract | ●●● (wards mapped) | ●● uneven | ODbL | Nepal PBF tens of MB (**unverified**) | Yes, raw | Any | Raw, needs pipeline | Minutely | Very large |
| Wikidata | ●● | ●● | CC0 | > 100 GB dump (**unverified**) | Yes, raw | SPARQL | Hard | Continuous | Very large |
| geoBoundaries | ●●● polygons (ADM3) | ●●● polygons (to ADM5) | Mixed per layer | Medium | Yes | Any | Easy files | Irregular | Academic |
| GADM | ●●● | ●●● | Non-commercial | Medium | Not for us | Any | Easy files | Rare | Academic |
| Natural Earth | ● | ● | Public domain | Small | Yes | Any | Easy | Stalled | Medium |
| OpenAddresses | ○ | ○ | Per source | Large | Yes | Any | Medium | Rolling | Medium |
| Nominatim | ●●● (as OSM) | ●● | GPL-3.0 | ≥ 1 TB planet | Server only | HTTP | Good API, hard ops | Releases 2–3 a year; minutely data | Large |
| Photon | ●●● (as OSM) | ●● | Apache-2.0 | ~95 GB planet | Server only | HTTP | Good | Frequent | Medium |
| Pelias | ●● (WOF admin) | ●● | MIT | Tens of GB (**unverified**) | Server only | HTTP | Complex ops | Slow | Small core team |
| libpostal | n/a (parser) | n/a | MIT | ~1.8 GB | Yes, too big for mobile | C, many wrappers | Good | Rare | Medium |
| reverse_geocoder & kin | ● nearest city | ● nearest city | LGPL / MIT | 1–30 MB | Yes | One language each | Trivial | Frozen | Fragmented |
| tzf-rs / timezonefinder / geo-tz | Timezone only | Timezone only | MIT + ODbL data | ~MBs (**unverified** exact) | Yes | Rust, Go, Py, JS | Good | Follows tz releases | Small, active |
| country-coder / country-boundaries | ○ (country) | ○ (country) | ISC / ODbL | < 1 MB | Yes | JS / Rust | Trivial | Rare | Small |
| OpenTopoData / Open-Elevation | Elevation only | Elevation only | MIT / GPL | DEM GBs | Self-host server | HTTP | Easy | Rare | Small |
| Google | ●● | ●●● | Proprietary, 30-day cache | n/a | No | All major | Excellent | Continuous | Huge |
| Mapbox | ● to ●● | ●● | Proprietary | n/a | Tiles only; search premium | Mobile SDKs, JS | Very good | Continuous | Large |
| HERE / TomTom / Esri / Apple | ● to ●● (**unverified**) | ●● | Proprietary | n/a | Navigation SDKs only | SDKs | Good | Continuous | Enterprise |
| Baato | ●●● (claimed) | ○ | Proprietary | n/a | No (app only) | REST | Fair | Unknown | Local |
| Galli Maps | ●●● (claimed) | ○ | Proprietary | n/a | No | REST, community Flutter | Poor docs (users) | Unknown | Local |
| Mappls | ○ / ● | ●●● (claimed) | Proprietary | n/a | Embedded nav only | REST, SDKs | Fair | Continuous | Large in India |
| Ola Maps | ○ | ●● to ●●● (claimed) | Proprietary | n/a | No | REST, SDKs | Fair | Continuous | Growing |
| **teigeodata (target)** | **●●● ward + tole** | **●●● village + ward** | **Apache-2.0 code; ODbL / CC BY data** | **Nepal < 10 MB core** | **Yes, signed, delta** | **13+ languages + MCP** | **Goal: < 1 h integration** | **Monthly CalVer** | **To build** |

### 4.2 Verdict per class

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| GeoNames | Ubiquitous, simple, CC BY | Duplicates, rounding, stale Nepal hierarchy | CC BY 4.0 | Daily dumps | Input and public baseline to beat |
| Overture | Monthly releases, GERS, corporate backing | Admin depth spotty, places noisy, ODbL divisions, ID churn | ODbL / per source | 2026-10-21.0 | Input; biggest strategic risk; copy the release model |
| WOF | Best ID model, placetype ladder | Slow updates, heavy repos | CC0 + per source | Rolling | Copy ID design; filtered input |
| Nominatim / Photon | Accurate where OSM is good; best ranking | Server-only, very large | GPL-3.0 / Apache-2.0 | 5.3.3 / active | Accuracy baseline; output-compatibility target |
| Pelias | Modular, multi-source | Ops-heavy, small team | MIT | Active, slow | Copy PIP + Placeholder split |
| Embedded reverse geocoders | Tiny, trivial API | Nearest city only; stale | LGPL / MIT | Mostly frozen | Displace: our "nano" pack must be as easy |
| tz libraries | Exact, fast | Timezone only | MIT + ODbL | Active | Match accuracy and speed; beat on integration |
| Google and peers | Coverage, polish | Cost, 30-day cache, online | Proprietary | Mature | Accuracy reference only, within terms |
| Regional (Baato, Galli, Mappls, Ola) | Local knowledge | Closed, online, no published accuracy | Proprietary | Young to mature | Potential data partners or corrections sources; benchmark where terms allow |

## 5. What we build ourselves, and why

### 5.1 Gaps we can own

1. **Lowest-level admin, Nepal and India, offline.** Ward and tole in Nepal, village and town and ward in India, with polygons at several levels of detail. Nobody open has it cleaned; GADM has it but is non-commercial; Overture says "spotty"; commercial players will not let you keep it.
2. **Published accuracy.** No rival publishes P50/P90 position error, elevation error or recall per country. Google, Mappls and Galli claim accuracy; GeoNames disclaims nothing. We publish per release, per country.
3. **Cross-script local search offline.** काठमाडौं / Kathmandu / Kathamandu / Kantipur in a few MB. Photon dumps carry only en/de/fr plus local; embedded libraries have no search at all.
4. **"Keep the data" licence.** Every commercial API limits storage. OpenCage is the only API that says "store forever", and it is online. We give the whole database under ODbL or CC BY.
5. **One answer across languages.** Every embedded library lives in one ecosystem (Python, npm, pub.dev, crates.io). A parity-gated API across 13+ languages, plus MCP, is unique.
6. **Point → everything in one call.** Reverse to the lowest admin unit plus timezone plus elevation plus postal code plus population, offline. Today a developer stitches reverse_geocoder + timezonefinder + OpenTopoData + a postcode CSV.
7. **Dynamic regional downloads with signatures and deltas.** Mapbox has tile regions for maps only; nobody has it for a gazetteer.
8. **Nepali postal codes (current and former), DIGIPIN encode/decode, and the Nepal 2017 restructuring as validity periods.** Small, high-value, nobody has them together.

### 5.2 Features we must match to be credible

| Feature | Who sets the bar | Our minimum |
|---|---|---|
| Prefix and typo-tolerant search with ranking | Photon, Nominatim, Google Autocomplete | Recall@10 at least Photon's on our Nepal test set |
| Reverse geocode to containing polygon | Nominatim, Pelias PIP, Google | Exact containment, never nearest-point fallback silently |
| Exact IANA timezone | tzf-rs, timezonefinder, geo-tz | 100% agreement with timezone-boundary-builder on main |
| Elevation lookup | Google Elevation, OpenTopoData | Same DEM class (Copernicus GLO-30) plus uncertainty |
| HTTP API shape | Nominatim / Pelias / Google | Self-hosted server with Nominatim-compatible JSON option |
| Country / ISO lookups in < 1 µs | country-coder, country-boundaries | Equal or faster |
| Stable IDs and redirects | WOF, Overture GERS | Never reuse; redirects on merge; tombstones |
| Monthly data releases with changelog | Overture | CalVer, changelog, diff per release |
| Mobile SDK ergonomics | Mapbox, Google | Flutter, Swift, Kotlin packages with one-line init |
| Attribution helpers | OSMF guidelines | Generated attribution string per pack set |

### 5.3 Benchmark targets

What we measure, against whom, and the bar. All runs reproducible from the benchmark harness; numbers published per release.

| Measure | Against | Test set | Target for Nepal v1 |
|---|---|---|---|
| Pack size, Nepal places + hierarchy + names | GeoNames `NP.zip` (3.5 MB zip, 2026-11-06); `reverse_geocoder` bundled data | — | Core ≤ 10 MB; "nano" (points + names + hierarchy) ≤ 5 MB (**target, to confirm with format note**) |
| Pack size, India | GeoNames `IN.zip` (15.8 MB) | — | Core ≤ 60 MB (**target**) |
| Position error P50 / P90, settlements | GeoNames, Overture divisions, OSM nodes, Wikidata | Survey ground truth + DoS points ([nepal-sources.md](nepal-sources.md)) | P90 better than GeoNames by ≥ 2× |
| Reverse admin correctness (lowest unit) | Nominatim, Overture divisions, Google (sample within terms) | Points with known ward | ≥ 99% at local level, ≥ 97% at ward |
| Search recall@1 / @10, Latin + Devanagari + misspellings | Photon, Nominatim, Google Autocomplete, Galli/Baato (if terms allow) | 2,000-query Nepal set with variants | recall@10 ≥ 95%, ≥ Photon |
| Duplicate rate | GeoNames, Overture places | Pairs within 2 km sharing a normalised name | < 0.5% |
| Timezone agreement | tzf-rs, timezonefinder, geo-tz | Random + border-dense points | 100% (main) |
| Elevation error | Google Elevation, OpenTopoData SRTM | ICESat-2 / survey points ([timezone-and-elevation.md](timezone-and-elevation.md)) | RMSE ≤ GLO-30 class, uncertainty calibrated |
| Reverse latency (p50/p99, warm) | `reverse_geocoder` (Python), `local-reverse-geocoder`, tzf-rs | 1 M random points | p50 ≤ 2 µs in Rust; ≤ 10× native in WASM |
| Search latency | Photon local, Nominatim local | Same query set | p99 ≤ 1 ms in-process |
| Cold start / memory | All embedded libraries | — | mmap open ≤ 5 ms; RSS ≤ pack size + small constant |
| Integration time | Google, Mapbox quick-starts | Timed walkthrough per language | ≤ 1 hour to Nepal offline |

## 6. Impact on the plan

- **Positioning page and README:** lead with "open, offline, keep it", Nepal depth and published accuracy. Name the rivals in a fair comparison table that links to the measured pages.
- **Benchmark harness (new roadmap item):** a reproducible suite against GeoNames, Overture divisions, Nominatim, Photon, tzf-rs, timezonefinder and OpenTopoData, with Google sampled only within its terms (no storage beyond 30 days, results deleted).
- **ADR (new): ID stability and redirects.** Adopt WOF-style immutable IDs with `supersedes` / `superseded_by`, plus Overture-style registry, changelog and bridge files (source ID → our ID, including GERS, WOF, GeoNames, OSM, Wikidata). Unlike Overture, keep old-to-new traceability on every split and merge.
- **ADR (new): compatibility outputs.** The HTTP server offers a Nominatim-compatible JSON mode and a GeoJSON mode, so existing clients switch with a URL change.
- **Format note:** confirm the pack-size targets in §5.3 against measured component sizes.
- **Names and search:** test set must include the queries in which Photon and Nominatim fail on Devanagari or variants, so the gap is measured, not claimed.
- **India wave:** DIGIPIN encode/decode as a small component (Apache-2.0 reference; reimplement from spec). Update [source-licences.md](source-licences.md): DIGIPIN README states Apache-2.0.
- **Partnerships to explore:** Kathmandu Living Labs (Baato), Galli Maps, DataMeet, OSM Nepal, as corrections sources and early users.

## 7. Risks and open items

**Risks.**
1. **Overture expands down the hierarchy.** If divisions fill locality and neighbourhood with good India and Nepal coverage, and places de-duplication improves, the "raw material" gap closes. Mitigation: ingest Overture as an input and keep a GERS bridge; our value is cleaning, depth, local names, offline packaging and measurement, not the raw layer. Watch the quarterly schema months.
2. **A funded offline SDK appears** (Mapbox offline search goes self-serve; Google or Apple ship on-device geocoding). Mitigation: licence ("keep it"), openness, multi-language parity, local depth.
3. **OSM licence friction.** ODbL share-alike on main makes some companies avoid it; the lite build must be good enough on its own. The Collective Database guideline does not cover merged, de-duplicated data, so main is a Derivative Database ([source-licences.md](source-licences.md)). Risk of user confusion about Produced Works vs. extracts. Mitigation: clear licence guide; lite as the default for cautious users.
4. **Boundary politics.** Kalapani / Lipulekh / Limpiyadhura (Nepal–India) and India's external boundary depiction. Mapbox handles this with `worldview`. We need a worldview mechanism before the India wave, or we will be blocked in one market or the other.
5. **Regional players close data further or object to benchmarking.** Read each one's terms before publishing any comparison.
6. **Maintainer bandwidth.** Pelias has two core maintainers, Gisgraphy stalled with one, Carmen vanished when its sponsor lost interest. A 13-language surface is a large maintenance load. Mitigation: generated bindings, parity gate, and a narrow core.
7. **ID churn of our own.** Overture's one-off 100% re-ID shows the cost. Decide the ID scheme before the first public pack and never change it.

**Open items (unverified, to close before public use).**
- HERE Base plan free tier and current platform terms text (plans page did not load).
- Google current Service Specific Terms wording and India prices (page did not render; dated versions used).
- TomTom, Mappls, Ola, Baato and Galli storage and benchmarking terms.
- Apple MapKit quotas and caching clause from Apple's own documentation.
- WOF per-country admin repo activity and Nepal/India depth; Overture divisions counts for NP and IN by subtype (run a DuckDB query against 2026-10-21.0).
- Pelias planet hardware figures; Photon planet dump download size; tzf-rs and timezonefinder data sizes.
- Natural Earth current version; GADM version.
- The Nepali geocoding study (NJG) year and method.
- what3words legal-threat history (primary source).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Nominatim | 5.3.3 (2026-10-14) | 2026-11-06 | GitHub releases API; nominatim.org |
| Photon | last push 2026-11-02 (release tag not retrieved) | 2026-11-06 | GitHub API |
| Pelias (meta / api) | last push 2026-07-24 / 2026-10-21 | 2026-11-06 | GitHub API |
| libpostal | last push 2026-05-13 | 2026-11-06 | GitHub API |
| `@mapbox/carmen` | 34.1.4 (2022-02-16); repo 404 | 2026-11-06 | npm registry; github.com |
| Gisgraphy | last push 2023-12-27 | 2026-11-06 | GitHub API |
| Overture | 2026-10-21.0 | 2026-11-06 | docs.overturemaps.org |
| GERS stability page | updated 2026-09-23 | 2026-11-06 | docs.overturemaps.org |
| GeoNames dumps | regenerated 2026-11-06 | 2026-11-06 | download.geonames.org (HEAD) |
| WOF data (umbrella repo) | last push 2024-03-06 | 2026-11-06 | GitHub API |
| geoBoundaries repo | last push 2026-11-01 | 2026-11-06 | GitHub API |
| OpenAddresses repo | last push 2026-11-05 | 2026-11-06 | GitHub API |
| natural-earth-vector repo | last push 2024-04-22 | 2026-11-06 | GitHub API |
| `reverse_geocoder` (PyPI) | 1.5.1 (2016-09-15) | 2026-11-06 | PyPI JSON API |
| `reverse_geocode` (PyPI) | 1.6.6 (2025-05-15) | 2026-11-06 | PyPI JSON API |
| `local-reverse-geocoder` | 0.16.8 (2026-06-14) | 2026-11-06 | npm registry |
| `offline-geocoder` (npm) | 1.0.0 (2018-08-02) | 2026-11-06 | npm registry |
| `@rapideditor/country-coder` | 5.6.1 (2026-01-31) | 2026-11-06 | npm registry |
| `which-polygon` | 2.2.1 (2023-02-16) | 2026-11-06 | npm registry |
| `geo-tz` | 8.1.11 (2026-11-02) | 2026-11-06 | npm registry |
| `@photostructure/tz-lookup` | 11.8.0 (2026-11-05) | 2026-11-06 | npm registry |
| `timezonefinder` | 9.2.0 (2026-10-22) | 2026-11-06 | PyPI |
| `tzf-rs` | 2.3.0 (2026-11-02) | 2026-11-06 | crates.io API |
| `reverse_geocoder` (crate) / `rgeo` / `country-boundaries` | 4.1.1 / 0.3.0 / 1.2.0 | 2026-11-06 | crates.io API |
| `geocoding` (Flutter) / `geocoder_offline` / `country_coder` (pub) | 5.0.1 / 0.2.1 / 1.2.0 | 2026-11-06 | pub.dev API |
| Open-Elevation / OpenTopoData | last push 2024-04-17 / 2026-02-16 | 2026-11-06 | GitHub API |
| Google Maps Platform pricing | live page | 2026-11-06 | developers.google.com |
| Mapbox pricing and Geocoding docs | live pages | 2026-11-06 | mapbox.com, docs.mapbox.com |
| Esri Location Platform pricing | live page | 2026-11-06 | location.arcgis.com |
| TomTom pricing | live page | 2026-11-06 | docs.tomtom.com |
| OpenCage pricing | live page | 2026-11-06 | opencagedata.com |
| Baato pricing | live page | 2026-11-06 | baato.io |
| Ola Maps pricing | live page | 2026-11-06 | maps.olakrutrim.com |
| DIGIPIN | README states Apache-2.0 | 2026-11-06 | github.com/INDIAPOST-gov/digipin |

## 9. Sources

- [GeoNames about](https://www.geonames.org/about.html) and [export / web services](https://www.geonames.org/export/), GeoNames, accessed 2026-11-06.
- [GeoNames dumps](https://download.geonames.org/export/dump/), headers read 2026-11-06.
- Ahlers, D., [Assessment of the accuracy of GeoNames gazetteer data](https://www.informatik.uni-oldenburg.de/~there/ahlers2013girgeonames.pdf), GIR '13, 2013.
- [Who's On First](https://whosonfirst.org/), accessed 2026-11-06; [Geocode Earth WOF data](https://geocode.earth/data/whosonfirst/), accessed 2026-11-06.
- [Overture GERS](https://docs.overturemaps.org/gers/) (updated 2026-09-22) and [GERS ID stability](https://docs.overturemaps.org/gers/stability/) (updated 2026-09-23), Overture Maps Foundation, accessed 2026-11-06.
- [Overture places guide](https://docs.overturemaps.org/guides/places/), [divisions guide](https://docs.overturemaps.org/guides/divisions/), [addresses guide](https://docs.overturemaps.org/guides/addresses/), accessed 2026-11-06.
- [geoBoundaries](https://www.geoboundaries.org/), William & Mary geoLab, accessed 2026-11-06.
- [GADM licence](https://gadm.org/license.html), accessed 2026-11-06 (via [source-licences.md](source-licences.md)).
- [Natural Earth blog](https://www.naturalearthdata.com/blog/), accessed 2026-11-06.
- [Nominatim installation](https://nominatim.org/release-docs/latest/admin/Installation/), Nominatim 5.3.3 manual, accessed 2026-11-06.
- [Photon](https://github.com/komoot/photon), komoot, accessed 2026-11-06.
- [Pelias](https://github.com/pelias/pelias), Geocode Earth, accessed 2026-11-06.
- [@mapbox/carmen on npm](https://www.npmjs.com/package/@mapbox/carmen), accessed 2026-11-06; [headwaymaps/carmen fork](https://www.github.com/headwaymaps/carmen).
- [libpostal](https://github.com/openvenues/libpostal), accessed 2026-11-06; [What is libpostal](https://senzing.com/what-is-libpostal/), Senzing (secondary, model sizes).
- [Gisgraphy](https://github.com/gisgraphy/gisgraphy), accessed 2026-11-06.
- [OpenCage pricing](https://opencagedata.com/pricing), accessed 2026-11-06.
- [reverse-geocoder](https://github.com/thampiman/reverse-geocoder), [PyPI](https://pypi.org/project/reverse_geocoder/); [local-reverse-geocoder](https://github.com/tomayac/local-reverse-geocoder); [geocoder_offline](https://pub.dev/packages/geocoder_offline); [geocoding (Flutter)](https://pub.dev/packages/geocoding); [country-coder](https://github.com/rapideditor/country-coder); [which-polygon](https://github.com/mapbox/which-polygon); registries accessed 2026-11-06.
- [tzf-rs](https://crates.io/crates/tzf-rs), [timezonefinder](https://pypi.org/project/timezonefinder/), [node-geo-tz](https://github.com/evansiroky/node-geo-tz), accessed 2026-11-06 (detail in [timezone-and-elevation.md](timezone-and-elevation.md)).
- [Android Geocoder reference](https://developer.android.com/reference/android/location/Geocoder), Google, accessed 2026-11-06.
- [Is CLGeocoder free or do we need to pay above 25,000 calls daily?](https://developer.apple.com/forums/thread/736348) and [MapKit JS quota limit](https://developer.apple.com/forums/thread/823245), Apple Developer Forums, accessed 2026-11-06 (forum, secondary).
- [Open Topo Data](https://www.opentopodata.org/), accessed 2026-11-06; [Open-Elevation](https://github.com/Jorl17/open-elevation).
- [Google Maps Platform pricing](https://developers.google.com/maps/billing-and-pricing/pricing), accessed 2026-11-06; [India pricing](https://developers.google.com/maps/billing-and-pricing/pricing-india).
- [Google Maps Platform Service Specific Terms](https://cloud.google.com/maps-platform/terms/maps-service-terms) and dated version [2024-05-22](https://cloud.google.com/maps-platform/terms/maps-service-terms/index-20240522), Google, accessed 2026-11-06.
- [Geocoding API policies](https://developers.google.com/maps/documentation/geocoding/policies), Google, accessed 2026-11-06.
- [Map Wars: Google offers free access of up to $6,800 to Indian developers](https://inc42.com/buzz/map-wars-google-offers-free-access-of-up-to-6800-to-indian-developers), Inc42, 2024.
- [Mapbox pricing](https://www.mapbox.com/pricing), [Geocoding API](https://docs.mapbox.com/api/search/geocoding/), [Offline search (Android)](https://docs.mapbox.com/android/search/guides/search-engine/offline), [Offline concepts](https://docs.mapbox.com/android/maps/guides/offline/), Mapbox, accessed 2026-11-06.
- [Permanent Geocoding: overview, licensing and usage rules](https://docs.here.com/here-kb/docs/permanent-geocoding-overview-licensing-and-usage-rules), HERE, accessed via search 2026-11-06.
- [TomTom pricing](https://docs.tomtom.com/pricing), accessed 2026-11-06.
- [ArcGIS Location Platform pricing](https://location.arcgis.com/pricing/), Esri, accessed 2026-11-06.
- [Can you store Places API results?](https://openplacesapi.com/blog/can-you-store-places-api-results), OpenPlacesAPI, validated 2026-08-04 (secondary).
- [what3words pricing](https://what3words.com/pricing) and [support: business costs](https://support.what3words.com/en/articles/2588610), accessed 2026-11-06.
- [DIGIPIN](https://github.com/INDIAPOST-gov/digipin), Department of Posts, accessed 2026-11-06; [India Post DIGIPIN](https://www.indiapost.gov.in/digipin).
- [MapmyIndia's MapplsPIN and India Post's DIGIPIN](https://www.medianama.com/2025/07/223-mapmyindias-mapplspin-india-posts-digipin/), MediaNama, July 2025.
- [Mappls APIs](https://about.mappls.com/api/), MapmyIndia, accessed 2026-11-06; [DIGIPIN conversion API](https://developer.mappls.com/documentation/sdk/rest-apis/mappls-digipin-conversion-api/readme).
- [Ola Maps pricing](https://maps.olakrutrim.com/pricing) and [Fair Usage Policy 2026](https://maps.olakrutrim.com/legal-docs/fair-usage-policy-2026.pdf), Krutrim, accessed 2026-11-06; [Ola Maps: made for India, priced for India](https://tech.olakrutrim.com/ola-maps-made-for-india-priced-for-india/), Krutrim, 2024.
- [Baato pricing](https://baato.io/pricing) and [geocoding](https://baato.io/products/geocoding), Kathmandu Living Labs, accessed 2026-11-06.
- [Galli Maps wins Startup World Cup Nepal Regional 2023](https://www.b360nepal.com/galli-maps-wins-startup-world-cup-nepal-regional-2023), B360, 2023; [Digital e-Palika and Galli Maps collaboration](https://ictframe.com/digital-e-palika-and-galli-maps-collaboration), ICT Frame; [galli_map package](https://pub.dev/packages/galli_map).
- [Map API (Galli Maps thread)](https://www.traccar.org/forums/topic/map-api/), Traccar forum, about 2024.
- [Nepali-made location apps gain credence for their greater accuracy](https://kathmandupost.com/money/2023/03/06/nepali-made-location-apps-gain-credence-for-their-greater-accuracy), The Kathmandu Post, 2023-03-06.
- [Google Maps in Kathmandu](https://theannapurnaexpress.com/story/14487/), The Annapurna Express (opinion); [Gadgetbyte Nepal on map accuracy](https://www.gadgetbytenepal.com/?p=154220); [Dependence on Google caused errors: Aryal](https://thehimalayantimes.com/nepal/dependence-on-google-caused-errors-aryal/), The Himalayan Times, 2019.
- [Geocoding study, Nepalese Journal of Geography](https://nepjol.info/index.php/NJG/article/view/23004/19535), NepJOL (year not confirmed).
- [GitHub REST API](https://api.github.com/) repository metadata for all repos listed in §8, accessed 2026-11-06.
