# Command line (/docs/cli)

`tgd` is the command line of teigeodata: one static binary that looks inside packs, checks them,
cuts smaller ones, answers every query in text, JSON, GeoJSON or Nominatim's format, installs and
updates packs in the store, and serves them over [HTTP](/docs/servers/http) and
[MCP](/docs/servers/mcp).

<Callout title="Every example on this page ran">
The commands below are cut from one script, [tour.sh](https://github.com/teispace/teigeodata/blob/main/site/examples/cli/tour.sh),
which CI runs on the released pack `world-np-lite.tgd` and checks against the output shown.
</Callout>

## Install [#install]

```sh
cargo install --locked teigeodata-cli
```

It installs from crates.io; there are no prebuilt `tgd` binaries yet. `tgd completions zsh` (or `bash`,
`fish`, `powershell`, `elvish`) prints a completion script.

Every query takes `--pack <file>` (repeat it for several packs). Where packs disagree, the
[layer rule](/docs/concepts/packs#layers) decides, not their order: the newer release, then the
more specific pack. Without one, it answers from the packs installed in the [store](#the-store).

## Look inside a pack [#look-inside-a-pack]

```sh title="tour.sh"
tgd inspect "$PACK" | sed -n '1,4p'
```

```text title="Output"
world baseline 2026.11.14  (15641408 bytes, format 0.1, created 1970-01-01T00:00:00Z)
unsigned
licence: "cc-by-4.0"
attribution: 11 notices
```

The full listing goes on with every source's notice, every section with its component and size,
and the bytes of each component. `tgd verify <pack>` checks the head and every section's hash
(`--deep` also decodes every value, `--key` checks the signature); `tgd diff <old> <new>` lists
what changed between two releases, section by section and place by place. `tgd delta <old> <new>
-o <delta>` writes only the places that changed, as a pack read above the older release; it
refuses a pair it cannot express exactly.

## Search [#search]

A name, in any script or spelling. `-k` is how many:

```sh title="tour.sh"
tgd search Pokhara -k 3 --pack "$PACK"
```

```text title="Output"
Pokhara  settlement  28.266890,83.968000  tgd:np/1xhb  1006 m (model)  Asia/Kathmandu  score 132
Pokhara  admin 3  28.209407,83.981817  tgd:np/1xnr  842 m (model)  Asia/Kathmandu  score 105
Pokhara  settlement  28.063631,82.378569  tgd:np/15t1  612 m (model)  Asia/Kathmandu  score 90
```

Each line is the name, the kind (and admin level), the point, the ID, the elevation with how it
was found (`model` from the terrain, `surveyed` from a source), the timezone and the score.
Write a parent after a comma to rank the places inside it first:

```sh title="tour.sh"
tgd search "Kathmandu, Bagmati" -k 2 --pack "$PACK"
```

```text title="Output"
Kathmandu  settlement  27.701690,85.320600  tgd:np/2cn  1288 m (model)  Asia/Kathmandu  score 1144
Kathmandu  admin 3  27.708951,85.324957  tgd:np/2gm  1308 m (model)  Asia/Kathmandu  score 1136
```

`--focus lat,lon` ranks places near a point first: at the focus a place gains 30 points, and the
gain halves every `--scale-km` (50 by default). `--strict` ranks by how well the name matched
before anything else.

```sh title="tour.sh"
tgd search Shivapur -k 3 --focus 28.7,80.6 --scale-km 30 --pack "$PACK"
```

```text title="Output"
Shivapur  settlement  28.531014,80.821273  tgd:np/2v99  153 m (model)  Asia/Kathmandu  score 105
Shivapur  settlement  28.714055,80.986874  tgd:np/2raj  193 m (model)  Asia/Kathmandu  score 103
Shivapur  settlement  28.506089,81.303187  tgd:np/2rkx  179 m (model)  Asia/Kathmandu  score 96
```

## A place by its ID [#a-place-by-its-id]

```sh title="tour.sh"
tgd get tgd:np/1xhb --pack "$PACK"
```

```text title="Output"
Pokhara  settlement  28.266890,83.968000  tgd:np/1xhb  1006 m (model)  Asia/Kathmandu
```

Every query prints JSON with `--json`: one record per place, nulls where a value is unknown.

```sh title="tour.sh"
tgd get tgd:np/1xhb --json --pack "$PACK"
```

```json title="Output"
[
{"id":"tgd:np/1xhb","name":"Pokhara","kind":"settlement","admin_level":0,"lat":28.26689,"lon":83.968,"elevation":1006,"elevation_source":"model","parent":"tgd:np/1xnr","timezone":"Asia/Kathmandu"}
]
```

## From a point [#from-a-point]

`reverse` gives the nearest places with their distance and bearing (from the place to the point,
"3.2 km at 45° from Pokhara"); `inside` marks a place that holds the point: a unit by its
outline, a settlement or locality within its radius of the point. `--in-country` never crosses a border.

```sh title="tour.sh"
tgd reverse 27.7172 85.3240 -k 3 --pack "$PACK"
```

```text title="Output"
Narayanhiti Royal Palace  settlement  27.718504,85.320980  tgd:np/2h3  1309 m (model)  Asia/Kathmandu  330.8 m away at 116°  inside
Gairidhara  settlement  27.719775,85.326454  tgd:np/2h2  1306 m (model)  Asia/Kathmandu  374.6 m away at 220.2°
Naxal  settlement  27.714251,85.326034  tgd:np/2gh  1308 m (model)  Asia/Kathmandu  384.2 m away at 328.6°
```

`near` does the same from a place:

```sh title="tour.sh"
tgd near tgd:np/2cn -k 3 --pack "$PACK"
```

```text title="Output"
Bhrikuti Mandap  settlement  27.703020,85.319798  tgd:np/2ga  1298 m (model)  Asia/Kathmandu  167.6 m away at 151.9°
Bagbazar  settlement  27.704485,85.318880  tgd:np/2g9  1298 m (model)  Asia/Kathmandu  353.9 m away at 151.4°
Shahidgate  settlement  27.699626,85.317360  tgd:np/2cp  1295 m (model)  Asia/Kathmandu  393 m away at 54.3°
```

`admin` gives every unit that holds the point, country first:

```sh title="tour.sh"
tgd admin 28.2096 83.9856 --pack "$PACK"
```

```text title="Output"
Nepal  country NP  28.000000,84.000000  tgd:np/1wmc  1046 m (model)  Asia/Kathmandu
Gandaki  admin 1 NP-P4  28.384514,84.035774  tgd:np/2922  2689 m (model)  Asia/Kathmandu
Kaski  admin 2  28.345513,83.970886  tgd:np/295c  1231 m (model)  Asia/Kathmandu
Pokhara  admin 3  28.209407,83.981817  tgd:np/1xnr  842 m (model)  Asia/Kathmandu
```

## Timezone and elevation [#timezone-and-elevation]

```sh title="tour.sh"
tgd tz 27.7172 85.3240 --pack "$PACK"
```

```text title="Output"
Asia/Kathmandu  timezone_area  28.397480,83.093537  tgd:np/1gce  Asia/Kathmandu  UTC+05:45 (+0545)
```

`tgd tz` also takes a place ID. The offset is computed from the tzdb rules inside the pack, at
the current time.

```sh title="tour.sh"
tgd elevation 27.7172 85.3240 --pack "$PACK"
```

```text title="Output"
1304.2 m above EGM2008 (NASADEM, 464 m spacing)
```

## GeoJSON and Nominatim [#geojson-and-nominatim]

`--geojson` prints a FeatureCollection (RFC 7946) ready for MapLibre, Leaflet or QGIS, with the
attribution at its top level:

```sh title="tour.sh"
tgd search Pokhara -k 1 --geojson --pack "$PACK"
```

```json title="Output"
{"type":"FeatureCollection","attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution","features":[
{"type":"Feature","id":"tgd:np/1xhb","geometry":{"type":"Point","coordinates":[83.968,28.26689]},"properties":{"id":"tgd:np/1xhb","name":"Pokhara","kind":"settlement","admin_level":0,"elevation":1006,"elevation_source":"model","parent":"tgd:np/1xnr","timezone":"Asia/Kathmandu","score":132}}
]}
```

`--nominatim` prints Nominatim's `json` format, so tools written for Nominatim read the output
unchanged:

```sh title="tour.sh"
tgd reverse 28.2096 83.9856 -k 1 --nominatim --pack "$PACK"
```

```json title="Output"
[
  {
    "address": {
      "admin_level_1": "Gandaki",
      "admin_level_2": "Kaski",
      "admin_level_3": "Pokhara",
      "country": "Nepal",
      "country_code": "np"
    },
    "boundingbox": [
      "28.210016",
      "28.210016",
      "83.987089",
      "83.987089"
    ],
    "display_name": "Prithvi-chowk, Pokhara, Kaski, Gandaki, Nepal",
    "lat": "28.210016",
    "licence": "Made with Natural Earth (https://www.naturalearthdata.com), public domain; GeoNames (https://www.geonames.org), CC BY 4.0; Time zones: IANA Time Zone Database 2026f (public domain); Wikidata (https://www.wikidata.org), CC0 1.0; Unicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3; Local Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in); Administrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0; Elevation: NASADEM, NASA JPL and LP DAAC; Buildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/); Buildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0; Central Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution",
    "lon": "83.987089",
    "name": "Prithvi-chowk",
    "place_id": "tgd:np/1xnp"
  }
]
```

## Cut a smaller pack [#cut-a-smaller-pack]

`extract` writes a pack of the places in a box (with their ancestors and timezone areas), of some
components, or both. The new pack is a normal pack: every query works on it.

```sh title="tour.sh"
tgd extract "$PACK" --bbox 85.2,27.6,85.5,27.8 --out kathmandu.tgd
tgd search Kathmandu -k 1 --pack kathmandu.tgd
```

```text title="Output"
wrote kathmandu.tgd: 428900 bytes, 14 sections, 1388 places; components point,elevation,admin,names,search,timezone,timezone-index,boundaries,admin-index,postal,dem,extra,tz-rules,substring,dem-coarse
Kathmandu  settlement  27.701690,85.320600  tgd:np/2cn  1288 m (model)  Asia/Kathmandu  score 144
```

`--components point,names` keeps only those components and what they need; with `--bbox`,
`--lod 3` stores coarser boundaries. `tgd export <pack>` streams every place out, in the order the pack stores
them.

## Postal codes [#postal-codes]

`tgd postal 44600` finds the places a code names; `--prefix` completes a partial code;
`tgd postal <lat> <lon>` gives the codes at a point. Codes are matched with or without
separators, in any case and in any script's digits.

## The store [#the-store]

The store keeps installed packs and the signed catalog that says which exist. It lives in the
platform's data directory, or where `--store` or `TEIGEODATA_STORE` says.

| Command | What it does |
|---|---|
| `tgd init` | Set up the store: the catalog's mirrors and its trusted root. |
| `tgd refresh [--countries a,b]` | Fetch what changed in the catalog. |
| `tgd catalog` | List the catalog's packs, offline. |
| `tgd find <name>` | Find a place by name with the packs that hold it, offline: what to install for "Dubai". |
| `tgd locate <lat> <lon>` | The countries at a point, with their packs. |
| `tgd install <packs>` | Install packs and their dependencies; `--components`, `--names` and `--outlines` install a part. |
| `tgd update` | Update installed packs to the catalog's releases. |
| `tgd remove`, `tgd pin` | Remove a pack (and what nothing else needs), or keep one when room is made. |
| `tgd status`, `tgd repair` | The installed packs and their health; fetch what a damaged one lacks. |

The store reads the live signed catalog: its addresses and the trust anchor are on
[the store](/docs/concepts/store) page. A pack downloaded by hand still opens with `--pack`.

```sh
curl -LO https://raw.githubusercontent.com/teispace/teigeodata/catalog/main/1.root.json
tgd init --repo https://raw.githubusercontent.com/teispace/teigeodata/catalog/main/ \
  --assets https://github.com/teispace/teigeodata/releases/download/ --root 1.root.json
tgd refresh --countries np
tgd install np --components point,names,search
tgd search Pokhara
```

## Serve [#serve]

`tgd serve` answers every query over HTTP, and `tgd mcp` over MCP for AI agents: see
[HTTP server](/docs/servers/http) and [MCP server](/docs/servers/mcp).