# India (/docs/data/countries/in)

<Callout title="Generated from the data">
This page is written by `cargo xtask data-pages` from [`data/countries/in`](https://github.com/teispace/teigeodata/tree/main/data/countries/in) and the pack the site's
examples run on (`world-np-lite.tgd`, data release `data-2026.11.14.3`), and CI fails when it differs. It changes with
each release.
</Callout>

## In the pack [#in-the-pack]

`world-np-lite.tgd` holds **8,111 places** in India: those whose country in the hierarchy is India,
as [disputed borders](/docs/concepts/accuracy#boundaries-and-perspective) are drawn. Their IDs begin `tgd:in/`,
except places a disputed border moved, which keep the ID they were first given.

| Kind | Places | With a height |
|---|--:|--:|
| Country | 1 | 0.0% |
| Admin units | 800 | 0.0% |
| Settlements | 7,310 | 100.0% |

## Admin levels [#admin-levels]

The ladder is the country's own, from its profile. The counts are the admin places of each level
in the pack: the current units, and former ones kept so their names still resolve (only current
units have an ISO code).

| Level | Unit | Local name | In the pack | With an ISO 3166 code |
|--:|---|---|--:|--:|
| 0 | Country |  | 1 | 1 |
| 1 | State or union territory |  | 38 | 36 |
| 2 | District |  | 762 | 0 |
| 3 | Sub-district |  | 0 | 0 |
| 4 | Village or town |  | 0 | 0 |

## Heights and timezones [#heights-and-timezones]

7,307 of 8,111 places have a height, in metres above EGM2008: the lowest -1 m, the median
198 m, the highest 3681 m. How heights are found and how good they are: [accuracy](/docs/concepts/accuracy).

| Timezone | Places |
|---|--:|
| `Asia/Dhaka` | 1 |
| `Asia/Karachi` | 30 |
| `Asia/Kolkata` | 8,079 |
| `Asia/Thimphu` | 1 |

## Names [#names]

Languages (BCP 47): `hi`, `en`. Scripts (ISO 15924): `Deva`, `Latn`, `Beng`, `Guru`, `Gujr`, `Orya`, `Taml`, `Telu`, `Knda`, `Mlym`, `Arab`, `Olck`, `Mtei`.

### Lexicon [#lexicon]

The spellings of generic terms in names, each with its evidence in
[`lexicon.yaml`](https://github.com/teispace/teigeodata/blob/main/data/countries/in/lexicon.yaml):
21 terms, 0 accepted. A name is written with the canonical spelling, and search finds
it by every variant.

| Term | Local | Variants | Meaning | Kind | Status |
|---|---|---|---|---|---|
| Chowk | चौक | Chauk, Chok | square (Chandni Chowk) | generic | proposed |
| Bazar | बाज़ार, बाजार | Bazaar, Bajar | market | generic | proposed |
| Gaon | गाँव, गाव | Gaun, Gaanv, Gav | village (Goregaon) | generic | proposed |
| Garh | गढ़ | Gadh, Gad | fort (Chandigarh) | generic | proposed |
| Garhi | गढ़ी | Gadhi | small fort | generic | proposed |
| Pur | पुर | Poor, Pore | town (Jaipur, older Cawnpore) | generic | proposed |
| Nagar | नगर | Nagara | town | generic | proposed |
| Ganj | गंज | Gunj | market (Daryaganj) | generic | proposed |
| Abad | आबाद | Abaad, Bad | settled (Ahmedabad) | generic | proposed |
| Bagh | बाग़ | Baag, Bag | garden | generic | proposed |
| Mandi | मंडी, मण्डी | Mundi | market | generic | proposed |
| Pura | पुरा |  | quarter, hamlet | generic | proposed |
| Puri | पुरी |  | town, city | generic | proposed |
| Khera | खेड़ा | Kheda, Kheraa | hamlet | generic | proposed |
| Kalan | कलाँ | Kalaan | greater (the larger of twin villages) | qualifier | proposed |
| Khurd | खुर्द | Khurad | lesser (the smaller of twin villages) | qualifier | proposed |
| Palli | పల్లి | Pally, Palle | village (Telugu) | generic | proposed |
| Halli | ಹಳ್ಳಿ | Hally | village (Kannada) | generic | proposed |
| Peth | पेठ | Peit | market quarter (Marathi) | generic | proposed |
| Pet | ಪೇಟೆ | Pete | market quarter (Kannada) | generic | proposed |
| Wadi | वाडी | Vadi, Wari, Vaadi | hamlet | generic | proposed |

## Sources [#sources]

The sources pinned for India in
[`data/sources.lock`](https://github.com/teispace/teigeodata/blob/main/data/sources.lock), by version and SHA-256. The
main build may read any of them that its recipe names; the lite build only those under an open
licence that asks no more than attribution, by the class the gate reads (`CLEAN_ROOM.md`). The world sources every country
shares are on the [data page](/docs/data#sources).

| Source | Version | Licence | Class | Lite build |
|---|---|---|---|---|
| [GeoNames India dump (IN.zip)](https://download.geonames.org/export/dump/IN.zip) | daily dump of 2026-11-12 | CC-BY-4.0 | `cc-by` | may read it |
| [LGD: India's 36 states and union territories with their LGD and Census codes (Parquet, daily LGD mirror)](https://github.com/ramSeraph/opendata/releases/download/lgd-parquet-latest/states.2026-11-12.f266902f935d8f73.parquet) | LGD report observed 2026-11-12 (mirror release lgd-parquet-latest; its .json beside the file records LGD's headers and the same SHA-256) | LGD copyright policy: reproduction free of charge with the source acknowledged, accurately and not in a misleading context | `godl` | may read it |
| [LGD: India's 5,050 urban local bodies (statutory towns): names in English and the state's language, type, Census 2011 code (Parquet, daily LGD mirror)](https://github.com/ramSeraph/opendata/releases/download/lgd-parquet-latest/urban_local_bodies.2026-11-12.e8deb348d470ed9f.parquet) | LGD report observed 2026-11-12 (mirror release lgd-parquet-latest; its .json beside the file records LGD's headers and the same SHA-256) | LGD copyright policy: reproduction free of charge with the source acknowledged, accurately and not in a misleading context | `godl` | may read it |
| [LGD: India's 7,112 sub-districts with their district and Census codes (Parquet, daily LGD mirror)](https://github.com/ramSeraph/opendata/releases/download/lgd-parquet-latest/subdistricts.2026-11-12.5b048fde2f5e2115.parquet) | LGD report observed 2026-11-12 (mirror release lgd-parquet-latest; its .json beside the file records LGD's headers and the same SHA-256) | LGD copyright policy: reproduction free of charge with the source acknowledged, accurately and not in a misleading context | `godl` | may read it |
| [LGD: India's 790 districts with their state and Census codes (Parquet, daily LGD mirror)](https://github.com/ramSeraph/opendata/releases/download/lgd-parquet-latest/districts.2026-11-12.21b9a610c65bdd21.parquet) | LGD report observed 2026-11-12 (mirror release lgd-parquet-latest; its .json beside the file records LGD's headers and the same SHA-256) | LGD copyright policy: reproduction free of charge with the source acknowledged, accurately and not in a misleading context | `godl` | may read it |
| [LGD: The districts, sub-districts and villages each urban local body covers (Parquet, daily LGD mirror)](https://github.com/ramSeraph/opendata/releases/download/lgd-parquet-latest/statewise_ulbs_coverage.2026-11-12.85cc0638d99c82a4.parquet) | LGD report observed 2026-11-12 (mirror release lgd-parquet-latest; its .json beside the file records LGD's headers and the same SHA-256) | LGD copyright policy: reproduction free of charge with the source acknowledged, accurately and not in a misleading context | `godl` | may read it |
| [Wikidata items with an LGD district code (P12746) and their GeoNames IDs (QLever snapshot)](https://qlever.dev/api/wikidata) | QLever Wikidata snapshot | CC0-1.0 | `cc0` | may read it |