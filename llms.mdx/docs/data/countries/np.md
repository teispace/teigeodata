# Nepal (/docs/data/countries/np)

<Callout title="Generated from the data">
This page is written by `cargo xtask data-pages` from [`data/countries/np`](https://github.com/teispace/teigeodata/tree/main/data/countries/np) and the pack the site's
examples run on (`world-np-lite.tgd`, data release `data-2026.11.14.3`), and CI fails when it differs. It changes with
each release.
</Callout>

## In the pack [#in-the-pack]

`world-np-lite.tgd` holds **95,716 places** in Nepal: those whose country in the hierarchy is Nepal,
as [disputed borders](/docs/concepts/accuracy#boundaries-and-perspective) are drawn. Their IDs begin `tgd:np/`,
except places a disputed border moved, which keep the ID they were first given.

| Kind | Places | With a height |
|---|--:|--:|
| Country | 1 | 100.0% |
| Admin units | 3,500 | 100.0% |
| Settlements | 90,371 | 100.0% |
| Localities | 878 | 100.0% |
| Natural features | 965 | 100.0% |
| timezone_area | 1 | 0.0% |

## Admin levels [#admin-levels]

The ladder is the country's own, from its profile. The counts are the admin places of each level
in the pack: the current units, and former ones kept so their names still resolve (only current
units have an ISO code).

| Level | Unit | Local name | In the pack | With an ISO 3166 code |
|--:|---|---|--:|--:|
| 0 | Country |  | 1 | 1 |
| 1 | Province | प्रदेश | 13 | 7 |
| 2 | District | जिल्ला | 86 | 0 |
| 3 | Local level | स्थानीय तह | 779 | 0 |
| 4 | Ward | वडा | 2,622 | 0 |

## Heights and timezones [#heights-and-timezones]

95,715 of 95,716 places have a height, in metres above EGM2008: the lowest 57 m, the median
1145 m, the highest 8848 m. How heights are found and how good they are: [accuracy](/docs/concepts/accuracy).

| Timezone | Places |
|---|--:|
| `Asia/Kathmandu` | 95,693 |

## Names [#names]

Languages (BCP 47): `ne`, `en`, `new`. Scripts (ISO 15924): `Deva`, `Latn`, `Newa`.
Names with no Latin spelling are romanised by `ne-Deva`; names missing in a script are written by `ne-Latn-Deva` and marked as transliterations, never canonical.

### Lexicon [#lexicon]

The spellings of generic terms in names, each with its evidence in
[`lexicon.yaml`](https://github.com/teispace/teigeodata/blob/main/data/countries/np/lexicon.yaml):
43 terms, 35 accepted. A name is written with the canonical spelling, and search finds
it by every variant.

| Term | Local | Variants | Meaning | Kind | Status |
|---|---|---|---|---|---|
| Chowk | चोक, चौक | Chok, Chouk, Chauk | square, crossroads | generic | accepted |
| Bazar | बजार | Bajar, Bazaar, Bajaar | market town | generic | accepted |
| Gaun | गाउँ | Gaon, Gau, Gaaun | village | generic | accepted |
| Tole | टोल | Tol | neighbourhood | generic | accepted |
| Danda | डाँडा | Dada, Daanda, Dandaa | ridge | generic | accepted |
| Khola | खोला | Kola, Kholaa | river, stream | generic | accepted |
| Kot | कोट | Kote, Kotte | fort, hill fort | generic | accepted |
| Kharka | खर्क | Kharkha, Kharaka | high pasture | generic | accepted |
| Ghat | घाट | Ghaat | riverbank steps | generic | accepted |
| Besi | बेंसी, बेसी | Beshi, Bensi, Beshee | valley floor | generic | accepted |
| Phedi | फेदी | Pedi, Fedi, Phedee | foot of a hill | generic | accepted |
| Bhanjyang | भञ्ज्याङ, भन्ज्याङ | Bhanjang, Bhanjyan, Bhanjyaang, Bhangyang | pass | generic | accepted |
| Deurali | देउराली | Dewrali, Deorali, Deuraali | pass with shrine | generic | accepted |
| Pokhari | पोखरी | Pokhri, Pokhree, Pokhary | pond | generic | accepted |
| Thok | थोक | Thoke | (in toponyms) cluster, place | generic | proposed |
| Gadhi | गढी | Garhi, Gadi, Ghadi | fort | generic | accepted |
| Gadh | गढ | Garh, Gad | fort | generic | proposed |
| Tar | टार | Taar | river terrace | generic | accepted |
| Tal | ताल | Taal | lake | generic | accepted |
| Daha | दह | Dah, Dahaa | lake, pool | generic | accepted |
| Dobhan | दोभान | Dovan, Doban, Dobhaan | confluence | generic | accepted |
| Chaur | चौर | Chour, Chowr | meadow | generic | accepted |
| Chautara | चौतारा | Chautaara | resting platform (also a town) | generic | accepted |
| Chautari | चौतारी | Chautaari, Chautary | resting platform | generic | accepted |
| Bari | बारी | Baari | garden, field | generic | accepted |
| Pati | पाटी | Paati | rest house | generic | accepted |
| Dhunga | ढुङ्गा | Dhungaa, Dunga | stone | generic | accepted |
| Lek | लेक | Lekh | highland | generic | proposed |
| Himal | हिमाल | Himaal | snow range | generic | proposed |
| Dhara | धारा | Dhaara | water spout (Sundhara) | generic | proposed |
| Bahal | बहाल | Baha, Bahaal, Bahil | Newar monastery courtyard | generic | proposed |
| Than | थान, स्थान | Thaan, Sthan, Asthan | shrine site | generic | proposed |
| Marg | मार्ग | Marga, Maarg | road | street type | accepted |
| Path | पथ | Patha | road | street type | accepted |
| Galli | गल्ली | Gali | lane | street type | proposed |
| Nagar | नगर | Nagara, Nagr | town | generic | accepted |
| Pur | पुर | Poor, Pore | town | generic | accepted |
| Ganj | गञ्ज, गंज | Gunj, Gunge | market town | generic | accepted |
| Gaunpalika | गाउँपालिका | Gaupalika, Gaunpaalika | Rural Municipality | admin suffix | accepted |
| Nagarpalika | नगरपालिका | Nagarpaalika | Municipality | admin suffix | accepted |
| Upamahanagarpalika | उपमहानगरपालिका | Upa-mahanagarpalika | Sub-Metropolitan City | admin suffix | accepted |
| Mahanagarpalika | महानगरपालिका | Mahanagarpaalika | Metropolitan City | admin suffix | accepted |
| Ward | वडा | Wada, Woda, Wodaa | ward | admin suffix | accepted |

## Sources [#sources]

The sources pinned for Nepal in
[`data/sources.lock`](https://github.com/teispace/teigeodata/blob/main/data/sources.lock), by version and SHA-256. The
main build may read any of them that its recipe names; the lite build only those under an open
licence that asks no more than attribution, by the class the gate reads (`CLEAN_ROOM.md`). The world sources every country
shares are on the [data page](/docs/data#sources).

| Source | Version | Licence | Class | Lite build |
|---|---|---|---|---|
| [CBS geographical codes: Nepal's 753 local levels with English and Devanagari names (CSV, Open Data Nepal)](https://api.opendatanepal.com/dataset/22870675-146d-4e4f-8848-2216380170de/resource/e33aa0f3-72de-4074-a38b-c6a1bf55d9a8/download/tmpkzco5wr_.csv) | Open Data Nepal record, metadata modified 2026-06-14 | CC-BY | `cc-by` | may read it |
| [CBS geographical codes: Nepal's 77 districts with English and Devanagari names (CSV, Open Data Nepal)](https://api.opendatanepal.com/dataset/22870675-146d-4e4f-8848-2216380170de/resource/de973922-7b56-4521-b6a4-aa3de7f8d8a2/download/tmp8hp3gb3m.csv) | Open Data Nepal record, metadata modified 2026-06-14 | CC-BY | `cc-by` | may read it |
| [Copernicus DEM GLO-30 Public (DGED), Cloud Optimized GeoTIFF tiles over Nepal (AWS open data)](https://copernicus-dem-30m.s3.amazonaws.com/%7Btile%7D/%7Btile%7D.tif) | GLO-30 Public DGED, AWS COG (Float32, DEFLATE, predictor 3, PixelIsPoint) | Copernicus DEM licence | `proprietary` | no |
| [GeoNames Nepal dump (NP.zip)](https://download.geonames.org/export/dump/NP.zip) | daily dump | CC-BY-4.0 | `cc-by` | may read it |
| [Google Open Buildings v3 points, S2 level-6 cells over Nepal](https://storage.googleapis.com/open-buildings-data/v3/points_s2_level_6_gzip_no_header/) | v3 (inference May 2023, objects dated 2023-06-23), confidence >= 0.65 | CC-BY-4.0 | `cc-by` | may read it |
| [ICESat-2 ATL08 land segments over the Nepal box, 2023, subset by SlideRule (public atl08x endpoint)](https://sliderule.slideruleearth.io/source/atl08x) | ATL08 release 007 through SlideRule v5.8.0 (raw response streams: records wrapping GeoParquet) | NASA Earth science data, no restriction on use | `public-domain` | may read it |
| [Microsoft Global ML Building Footprints, Nepal (52 quadkey-9 files of GeoJSON lines)](https://minedbuildings.z5.web.core.windows.net/global-buildings/dataset-links.csv) | global-buildings 2026-02-03 partition, uploaded 2026-02-23 (Nepal 2019-2023 imagery) | CDLA-Permissive-2.0 | `cdla-permissive` | may read it |
| [NASADEM HGT v001, Cloud Optimized GeoTIFF tiles over Nepal (Microsoft Planetary Computer)](https://nasademeuwest.blob.core.windows.net/nasadem-cog/v001/%7Btile%7D.tif) | NASADEM_HGT v001 (Int16, DEFLATE, PixelIsPoint, EGM96 heights) | NASA data, no restriction on use | `public-domain` | may read it |
| [OCHA COD-AB Nepal v02, attribute tables (XLSX)](https://data.humdata.org/dataset/07db728a-4f0f-4e98-8eb0-8fa9df61f01c/resource/5bf18581-ef08-471c-b5d4-582c2d70228d/download/npl_admin_boundaries.xlsx) | v02, HDX resource of 2026-08-14 | CC-BY-IGO-3.0 | `cc-by-igo` | may read it |
| [OCHA COD-AB Nepal v02, boundaries (GeoJSON, zipped)](https://data.humdata.org/dataset/07db728a-4f0f-4e98-8eb0-8fa9df61f01c/resource/dea34e50-37d5-4e36-98ae-2b7b1b4c43de/download/npl_admin_boundaries.geojson.zip) | v02, HDX resource of 2026-08-14 | CC-BY-IGO-3.0 | `cc-by-igo` | may read it |
| [OpenStreetMap Nepal extract (Geofabrik), dated file](https://download.geofabrik.de/asia/nepal-261105.osm.pbf) | 2026-11-05 | ODbL-1.0 | `odbl` | no |
| [Postal Code Of Nepal, General Post Office (Department of Postal Services): the local-level and ward table, Internet Archive snapshot](https://web.archive.org/web/20261001113959id_/https://gpo.gov.np/pages/postal-code-1259614658/) | snapshot of 2026-10-01 (byte-identical table rows in the snapshots of 2025-09-08, 2026-01-09 and 2026-07-20) | none stated on the page | `proprietary` | no |
| [Small Area Estimation of Poverty 2023, National Statistics Office, Nepal: sheet "2. Municipal estimates", the 753 local levels' households and 2021 census population (xlsx)](http://113.199.192.53:82/app/home/download.php) | NSO Central Data Portal dataset 79, served 2026-11-23 | none stated on the portal | `proprietary` | no |
| [Wikidata places in Nepal with a coordinate (QLever snapshot)](https://qlever.dev/api/wikidata) | QLever Wikidata snapshot | CC0-1.0 | `cc0` | may read it |
| [WorldPop Global2 constrained population 2025, 100 m (3 arc-seconds), Nepal](https://data.worldpop.org/GIS/Population/Global_2015_2030/R2025A/2025/NPL/v1/100m/constrained/npl_pop_2025_CN_100m_R2025A_v1.tif) | R2025A v1 (alpha release, 2025-09), epoch 2025; Float32 BigTIFF, LZW, no-data -99999 | CC-BY-4.0 | `odbl` | no |