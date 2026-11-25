# HTTP server (/docs/servers/http)

`tgd serve` puts the packs behind an HTTP API: every query as a `GET` route, native JSON or
GeoJSON, and Nominatim-compatible `/search`, `/reverse` and `/lookup` so existing clients work
unchanged. It is one process with no database: the packs are memory-mapped, and on Nepal it
answers about 9,000 requests per second for the heaviest route (a large boundary) and 40,000 to
90,000 for the others, at 16 connections on one machine ([measured](/docs/data/measured#server-load)
by `cargo xtask load`, with a budget the load test enforces).

<Callout title="Every example on this page ran">
The requests below are part of the [command-line tour](https://github.com/teispace/teigeodata/blob/main/site/examples/cli/tour.sh),
which starts the server on the released pack, sends them with curl, and checks the answers.
</Callout>

## Start it [#start-it]

```sh
tgd serve --pack world-np-lite.tgd
# serving on http://127.0.0.1:8080
```

| Option | Default | What |
|---|---|---|
| `--listen <addr:port>` | `127.0.0.1:8080` | Where to listen; port 0 picks a free one. Loopback only unless you name another address. |
| `--pack <file>` | the store's packs | A pack to serve; repeat for several. Given, it replaces the store: only these packs are served, and nothing is followed. |
| `--store <dir>` | the platform's data directory | Serve the store's installed packs, and follow it as packs are installed or updated. |
| `--follow <seconds>` | 5 | How often to read the store's journal; 0 never. |
| `--timeout <seconds>` | 10 | A request taking longer is answered `unavailable`. |
| `--max-connections <n>` | 1024 | The most connections kept open; more wait until one closes. |
| `--max-in-flight <n>` | 256 | The most requests answered at once; one more is answered `unavailable` at once, with `Retry-After: 1`. |
| `--cors-origin <origin>` | none | An origin browsers may call from; repeat for several. CORS is off by default. |
| `--allow-install` | off | Let MCP clients install catalog packs into the store. |

```sh title="tour.sh"
curl -s "$URL/health"
```

```json title="Response"
{"status":"ok","layers":1}
```

## Native routes [#native-routes]

Every route is `GET /v1/<query>` with query parameters, and answers
`{"results": [...], "attribution": "..."}` (or `"result"` for one value). `limit` defaults to 10.

| Route | Answers |
|---|---|
| `/v1/search` | Places matching a name, best first, with `score`. |
| `/v1/get` | One place; `names=1` adds every name with its language. |
| `/v1/lookup` | Places by ID, in the order asked. |
| `/v1/reverse` | The nearest places with `meters`, `bearing` and `inside`. |
| `/v1/near` | The places nearest a place. |
| `/v1/admin` | The admin units holding the point, country first. |
| `/v1/hierarchy` | A place's ancestors, parent first. |
| `/v1/children` | A place's children. |
| `/v1/tz` | The timezone with its offset, abbreviation and DST flag at `t`. |
| `/v1/elevation` | The terrain height, its datum, source and grid spacing. |
| `/v1/distance` | The great-circle distance and initial bearing. |
| `/v1/postal` | Postal codes and their places. |
| `/v1/boundary` | A unit's outline as GeoJSON. |
| `/v1/coverage` | The catalog's packs that cover a point (a server over a store). |
| `/packs` | The packs served, with their components, places and attribution. |
| `/health` | `{"status": "ok", "layers": n}`. |

Every parameter of every route, with its bounds and default, is in the [HTTP API reference](/docs/reference/http).

Search:

```sh title="tour.sh"
curl -s "$URL/v1/search?q=Pokhara&limit=2"
```

```json title="Response"
{"results":[
{"id":"tgd:np/1xhb","name":"Pokhara","kind":"settlement","admin_level":0,"lat":28.26689,"lon":83.968,"elevation":1006,"elevation_source":"model","parent":"tgd:np/1xnr","timezone":"Asia/Kathmandu","score":132},
{"id":"tgd:np/1xnr","name":"Pokhara","kind":"admin","admin_level":3,"lat":28.209407,"lon":83.981817,"elevation":842,"elevation_source":"model","parent":"tgd:np/295c","timezone":"Asia/Kathmandu","score":105}
],"attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution"}
```

Reverse geocoding; `inside` is true when the place's area holds the point:

```sh title="tour.sh"
curl -s "$URL/v1/reverse?lat=27.7172&lon=85.3240&limit=1"
```

```json title="Response"
{"results":[
{"id":"tgd:np/2h3","name":"Narayanhiti Royal Palace","kind":"settlement","admin_level":0,"lat":27.718504,"lon":85.32098,"elevation":1309,"elevation_source":"model","parent":"tgd:np/2gm","timezone":"Asia/Kathmandu","meters":330.8,"bearing":116.0,"inside":true}
],"attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution"}
```

The timezone at a point, at an instant:

```sh title="tour.sh"
curl -s "$URL/v1/tz?lat=27.7172&lon=85.3240&t=1700000000"
```

```json title="Response"
{"results":[
{"id":"tgd:np/1gce","name":"Asia/Kathmandu","kind":"timezone_area","admin_level":0,"lat":28.39748,"lon":83.093537,"elevation":null,"parent":null,"timezone":"Asia/Kathmandu","utc_offset":"+05:45","tz_abbr":"+0545","is_dst":false}
],"attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution"}
```

Elevation and distance answer one value, under `result`:

```sh title="tour.sh"
curl -s "$URL/v1/elevation?lat=27.7172&lon=85.3240"
```

```json title="Response"
{"result":{"lat":27.7172,"lon":85.324,"elevation_m":1304.2,"datum":"EGM2008","source":"nasadem","resolution_m":463.8},"attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution"}
```

```sh title="tour.sh"
curl -s "$URL/v1/distance?from=27.7172,85.3240&to=tgd:np/1xhb"
```

```json title="Response"
{"result":{"bearing":295.0,"from":{"id":null,"lat":27.7172,"lon":85.324},"meters":146499.6,"to":{"id":"tgd:np/1xhb","lat":28.26689,"lon":83.968}},"attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution"}
```

## GeoJSON [#geojson]

Add `format=geojson` to any route that answers places, and the answer is a FeatureCollection
(RFC 7946) that MapLibre, Leaflet, OpenLayers and QGIS read directly:

```sh title="tour.sh"
curl -s "$URL/v1/admin?lat=28.2096&lon=83.9856&format=geojson"
```

```json title="Response"
{"type":"FeatureCollection","attribution":"Made with Natural Earth (https://www.naturalearthdata.com), public domain\nGeoNames (https://www.geonames.org), CC BY 4.0\nTime zones: IANA Time Zone Database 2026f (public domain)\nWikidata (https://www.wikidata.org), CC0 1.0\nUnicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3\nLocal Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)\nAdministrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0\nElevation: NASADEM, NASA JPL and LP DAAC\nBuildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)\nBuildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0\nCentral Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution","features":[
{"type":"Feature","id":"tgd:np/1wmc","geometry":{"type":"Point","coordinates":[84.0,28.0]},"properties":{"id":"tgd:np/1wmc","name":"Nepal","kind":"country","admin_level":0,"elevation":1046,"elevation_source":"model","parent":null,"timezone":"Asia/Kathmandu","iso_code":"NP"}},
{"type":"Feature","id":"tgd:np/2922","geometry":{"type":"Point","coordinates":[84.035774,28.384514]},"properties":{"id":"tgd:np/2922","name":"Gandaki","kind":"admin","admin_level":1,"elevation":2689,"elevation_source":"model","parent":"tgd:np/1wmc","timezone":"Asia/Kathmandu","iso_code":"NP-P4"}},
{"type":"Feature","id":"tgd:np/295c","geometry":{"type":"Point","coordinates":[83.970886,28.345513]},"properties":{"id":"tgd:np/295c","name":"Kaski","kind":"admin","admin_level":2,"elevation":1231,"elevation_source":"model","parent":"tgd:np/2922","timezone":"Asia/Kathmandu"}},
{"type":"Feature","id":"tgd:np/1xnr","geometry":{"type":"Point","coordinates":[83.981817,28.209407]},"properties":{"id":"tgd:np/1xnr","name":"Pokhara","kind":"admin","admin_level":3,"elevation":842,"elevation_source":"model","parent":"tgd:np/295c","timezone":"Asia/Kathmandu"}}
]}
```

```js title="MapLibre"
map.addSource("admin", { type: "geojson", data: `${server}/v1/admin?lat=28.2096&lon=83.9856&format=geojson` });
map.addLayer({ id: "admin", type: "circle", source: "admin" });
```

## Nominatim-compatible [#nominatim-compatible]

`/search`, `/reverse` and `/lookup` speak Nominatim's API for the parameters its clients use,
in the `json`, `jsonv2` and `geojson` formats (JSONP is refused: use CORS):

| Route | Parameters |
|---|---|
| `/search` | `q`, or the structured `amenity`, `street`, `city`, `county`, `state`, `country`, `postalcode`; `limit` (at most 40, as Nominatim); `countrycodes`; `viewbox` and `bounded`; `addressdetails` |
| `/reverse` | `lat`, `lon`, `zoom` (how coarse a place to answer); `addressdetails` (on by default) |
| `/lookup` | `osm_ids`: up to 50 teigeodata IDs, comma-separated |

Point a Nominatim client, a geocoder plugin or a script at teigeodata and it works offline:

```sh title="tour.sh"
curl -s "$URL/search?q=Pokhara&format=jsonv2&limit=1"
```

```json title="Response"
[{"boundingbox":["28.26689","28.26689","83.968","83.968"],"display_name":"Pokhara, Pokhara, Kaski, Gandaki, Nepal","lat":"28.26689","licence":"Made with Natural Earth (https://www.naturalearthdata.com), public domain; GeoNames (https://www.geonames.org), CC BY 4.0; Time zones: IANA Time Zone Database 2026f (public domain); Wikidata (https://www.wikidata.org), CC0 1.0; Unicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3; Local Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in); Administrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0; Elevation: NASADEM, NASA JPL and LP DAAC; Buildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/); Buildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0; Central Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution","lon":"83.968","name":"Pokhara","place_id":"tgd:np/1xhb"}]
```

A conformance corpus of real client requests is checked in CI. Differences from Nominatim, where
what cannot be filled is left out rather than faked:

- `place_id` is a teigeodata ID (a string); there is no `osm_type`, `osm_id`, `class`, `type` or
  `place_rank`, so `json` and `jsonv2` answer the same objects.
- `boundingbox` is the place's point (south = north, west = east), not its extent; draw an
  outline with the native `/boundary`.
- The address names the admin units by level as the country defines them.

## Errors [#errors]

A failure answers a stable code and an HTTP status:

```sh title="tour.sh"
curl -s -w '%{http_code}\n' "$URL/v1/reverse?lat=91&lon=0"
```

```json title="Response"
{"error":{"code":"invalid_argument","message":"91,0: the latitude is outside -90 to 90 degrees"}}
400
```

The codes and their statuses are listed in the [reference](/docs/reference/http#errors): `invalid_argument` and `out_of_bounds` answer 400 (a query string past the server's bound 414), `not_found` 404, `missing_component` 501, `unavailable` 503 and `internal` 500; `unauthenticated` 401, `forbidden` 403 and `rate_limited` 429 only from a deployment that adds keys, rules or limits.

## Deploying it [#deploying-it]

- **Bind it where you mean to.** It listens on loopback by default; behind a reverse proxy
  (Caddy, nginx), keep it there. With `--listen 0.0.0.0:8080` it answers anyone who can reach
  the port.
- **Limits are built in.** Every request is bounded (text length, `limit` at most 50, radius,
  time), so no request can make the server do unbounded work.
- **Floods and slow clients are bounded too.** Connections and requests in flight are capped.
  A request head must arrive within 10 seconds, and an idle HTTP/1 connection is closed after the
  same time. HTTP/2 peers are pinged every 20 seconds (and dropped when they stop answering) and
  get at most 32 streams each. A request past the in-flight
  cap is refused at once, never queued, so a client should retry after `Retry-After`. A reverse
  proxy in front still does TLS and per-client rate limits.
- **Updates without restarts.** Over a store, the server reads its journal every `--follow`
  seconds and serves newly installed or updated packs; queries in flight finish on the packs
  they started with.
- **Attribution** comes with every answer, from the packs that answered it; keep it visible
  where the data is shown.
- **MCP** is served at `/mcp` on the same port (Streamable HTTP): see [MCP server](/docs/servers/mcp).