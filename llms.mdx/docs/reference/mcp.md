# MCP tools (/docs/reference/mcp)

What `tools/list` and `resources/templates/list` give, with the bounds of a server's default limits. Every answer is structured content that its tool's output schema accepts, the same object as text, with the attribution under `_meta` too. How to run the server and connect a client: [MCP server](/docs/servers/mcp).

The server's instructions to a client:

> Offline place data. Find places by name with `search`, by coordinates with `reverse` and `admin_at`, around a place with `nearby`; climb with `hierarchy` and descend with `children`. Places are named by IDs such as tgd:np/1xhb: pass them to `get_place`, `lookup`, `nearby`, `timezone` or read teigeodata://place/\{id\}. Coordinates are WGS84 degrees, distances metres, heights metres above EGM2008. A long answer has a `next_cursor`: call the same tool with only `cursor` for the next page. Show the `attribution` of an answer wherever its data is shown: the licences require it.

## Tools [#tools]

| Tool | What |
|---|---|
| [`search`](#search) | Search places by name |
| [`reverse`](#reverse) | Places nearest a point |
| [`nearby`](#nearby) | Places near a place |
| [`get_place`](#get_place) | A place by ID |
| [`lookup`](#lookup) | Places by IDs |
| [`admin_at`](#admin_at) | Admin units at a point |
| [`hierarchy`](#hierarchy) | Ancestors of a place |
| [`children`](#children) | Children of a place |
| [`timezone`](#timezone) | Timezone |
| [`elevation`](#elevation) | Terrain height |
| [`postal`](#postal) | Postal codes |
| [`boundary`](#boundary) | Outline of a place |
| [`distance`](#distance) | Distance and bearing |
| [`packs`](#packs) | Installed packs |
| [`coverage`](#coverage) | Packs for a point |
| [`install`](#install) | Install packs |
| [`install_status`](#install_status) | How an install goes |

### `search` [#search]

**Search places by name.** Places matching a name, best first. "Name, Parent" ranks places under a parent of that name first. `focus` ranks places near a point higher (scale_km: how far counts as near, default 50). `strict` ranks by how well the name matched (exact, then loose, prefix, one edit) before importance and nearness.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `query` | string | yes | The name, in any script, such as Pokhara or पोखरा. 1 to 256 bytes. |
| `cursor` | string |  | The `next_cursor` of the previous page; the other arguments are then taken from it. |
| `focus` | object |  |  |
| `focus.lat` | number | yes | Latitude, WGS84 degrees. -90 to 90. |
| `focus.lon` | number | yes | Longitude, WGS84 degrees. -180 to 180. |
| `focus.scale_km` | number |  | Over 0; default `50`. |
| `limit` | integer |  | How many results, the page's size. 0 to 50; default `10`. |
| `strict` | boolean |  | Rank by match quality first. Default `false`. |

Answers [places](#places), a page at a time.

### `reverse` [#reverse]

**Places nearest a point.** The places nearest a point, nearest first, each with its distance in metres, its bearing, and whether its outline holds the point. `in_country`: only places in the country holding the point.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `lat` | number | yes | Latitude, WGS84 degrees. -90 to 90. |
| `lon` | number | yes | Longitude, WGS84 degrees. -180 to 180. |
| `cursor` | string |  | The `next_cursor` of the previous page; the other arguments are then taken from it. |
| `in_country` | boolean |  | Default `false`. |
| `limit` | integer |  | How many results, the page's size. 0 to 50; default `10`. |

Answers [places](#places), a page at a time.

### `nearby` [#nearby]

**Places near a place.** The places nearest a place, nearest first, the place itself left out; within `radius_m` metres when given.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `id` | string | yes | A place ID from an earlier answer, such as tgd:np/1xhb. 1 to 256 bytes. |
| `cursor` | string |  | The `next_cursor` of the previous page; the other arguments are then taken from it. |
| `limit` | integer |  | How many results, the page's size. 0 to 50; default `10`. |
| `radius_m` | number |  | Metres. 0 to 1000000.0. |

Answers [places](#places), a page at a time.

### `get_place` [#get_place]

**A place by ID.** One place by its ID; `names`: every name it has, with language and origin. A retired ID answers its survivor.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `id` | string | yes | A place ID from an earlier answer, such as tgd:np/1xhb. 1 to 256 bytes. |
| `names` | boolean |  | Default `false`. |

Answers [places](#places), a page at a time.

### `lookup` [#lookup]

**Places by IDs.** Places by ID, in the order asked; IDs not found are left out.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `ids` | array of string | yes | 1 to 50 items. |

Answers [places](#places), a page at a time.

### `admin_at` [#admin_at]

**Admin units at a point.** The admin units holding a point, the country first, then each finer level.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `lat` | number | yes | Latitude, WGS84 degrees. -90 to 90. |
| `lon` | number | yes | Longitude, WGS84 degrees. -180 to 180. |

Answers [places](#places), a page at a time.

### `hierarchy` [#hierarchy]

**Ancestors of a place.** A place's ancestors: its parent first, then the parent's, up to the country.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `id` | string | yes | A place ID from an earlier answer, such as tgd:np/1xhb. 1 to 256 bytes. |

Answers [places](#places), a page at a time.

### `children` [#children]

**Children of a place.** The places whose parent is a place, such as a country's provinces or a district's municipalities.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `id` | string | yes | A place ID from an earlier answer, such as tgd:np/1xhb. 1 to 256 bytes. |
| `cursor` | string |  | The `next_cursor` of the previous page; the other arguments are then taken from it. |
| `limit` | integer |  | How many results, the page's size. 0 to 50; default `10`. |

Answers [places](#places), a page at a time.

### `timezone` [#timezone]

**Timezone.** The IANA timezone of a place (`id`) or at a point (`lat`, `lon`), with its UTC offset, abbreviation and DST at instant `t` (Unix seconds, default now).

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `id` | string |  | A place ID from an earlier answer, such as tgd:np/1xhb. 1 to 256 bytes. |
| `lat` | number |  | Latitude, WGS84 degrees. -90 to 90. |
| `lon` | number |  | Longitude, WGS84 degrees. -180 to 180. |
| `t` | integer |  | Unix seconds. |

Answers [places](#places), a page at a time.

### `elevation` [#elevation]

**Terrain height.** The terrain height at a point, metres above the EGM2008 geoid, from the packs' elevation model; null where there is none.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `lat` | number | yes | Latitude, WGS84 degrees. -90 to 90. |
| `lon` | number | yes | Longitude, WGS84 degrees. -180 to 180. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | string or null |  |
| `result` | object |  |
| `result.datum` | string |  |
| `result.elevation_m` | number or null |  |
| `result.lat` | number |  |
| `result.lon` | number |  |
| `result.resolution_m` | number |  |
| `result.source` | string or null |  |

### `postal` [#postal]

**Postal codes.** Postal codes: a code to its places (`code`, optionally its `scheme` such as NP or IN-PIN), codes starting with `code` (`prefix`), or the codes at a point (`lat`, `lon`).

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `code` | string |  | 1 to 256 bytes. |
| `lat` | number |  | Latitude, WGS84 degrees. -90 to 90. |
| `limit` | integer |  | How many results, the page's size. 0 to 50; default `10`. |
| `lon` | number |  | Longitude, WGS84 degrees. -180 to 180. |
| `prefix` | boolean |  | Default `false`. |
| `scheme` | string |  | At most 256 bytes. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | string or null |  |
| `results` | array of object |  |
| `results[].admin_level` | integer |  |
| `results[].code` | string |  |
| `results[].id` | string |  |
| `results[].kind` | string |  |
| `results[].name` | string or null |  |
| `results[].replaced` | boolean |  |
| `results[].scheme` | string |  |
| `results[].valid_from` | integer or null |  |
| `results[].valid_to` | integer or null |  |

### `boundary` [#boundary]

**Outline of a place.** What a place's outline is: its level of detail (0 finest to 6 coarsest; the finest within the vertex bound when `lod` is not given), tolerance in metres, vertices, parts, holes and bounding box, and the `uri` of the outline itself as a GeoJSON resource, to read when it is drawn. A place with no outline answers `outlined: false`.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `id` | string | yes | A place ID from an earlier answer, such as tgd:np/1xhb. 1 to 256 bytes. |
| `lod` | integer |  | 0 to 6. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | string or null |  |
| `result` | object |  |
| `result.bbox` | array or null | West, south, east, north. 4 to 4 items. |
| `result.id` | string |  |
| `result.lod` | integer |  |
| `result.name` | string or null |  |
| `result.outlined` | boolean |  |
| `result.uri` | string |  |
| `result.vertices` | integer |  |
| `result.holes` | integer |  |
| `result.parts` | integer |  |
| `result.tolerance_m` | number |  |

### `distance` [#distance]

**Distance and bearing.** The great-circle distance in metres and initial bearing between two ends, each a place ID or a point as `lat,lon`.

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `from` | string | yes | A place ID such as tgd:np/1xhb, or a point such as 27.7172,85.324. 1 to 256 bytes. |
| `to` | string | yes | A place ID, or a point as `lat,lon`. 1 to 256 bytes. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | string or null |  |
| `result` | object |  |
| `result.bearing` | number | Degrees clockwise from north. |
| `result.from` | object |  |
| `result.from.id` | string or null |  |
| `result.from.lat` | number |  |
| `result.from.lon` | number |  |
| `result.meters` | number |  |
| `result.to` | object |  |
| `result.to.id` | string or null |  |
| `result.to.lat` | number |  |
| `result.to.lon` | number |  |

### `packs` [#packs]

**Installed packs.** The packs served: address, component, release, places, installed components and attribution.

Hints: read-only, idempotent.

No arguments.

Answers:

| Field | Type | What |
|---|---|---|
| `packs` | array of object |  |
| `packs[].address` | string |  |
| `packs[].attribution` | string or null |  |
| `packs[].component` | string |  |
| `packs[].components` | array of string |  |
| `packs[].places` | integer |  |
| `packs[].release` | string |  |

### `coverage` [#coverage]

**Packs for a point.** The countries at a point or within `radius_m` metres of it, each with the catalog's packs that could hold the point: path, release, download and disk bytes, components, parts, licence and state (installed, update or available). From the store's verified catalog, offline.

Listed only by a server over a store (`--store`).

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `lat` | number | yes | Latitude, WGS84 degrees. -90 to 90. |
| `lon` | number | yes | Longitude, WGS84 degrees. -180 to 180. |
| `radius_m` | integer |  | Metres; default 3,000. 0 to 1000000.0. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | string or null |  |
| `results` | array of object |  |
| `results[].countries` | array of string |  |
| `results[].id` | string |  |
| `results[].lat` | number |  |
| `results[].lon` | number |  |
| `results[].name` | string or null |  |
| `results[].packs` | array of object |  |
| `results[].packs[].components` | array of string |  |
| `results[].packs[].disk` | integer |  |
| `results[].packs[].download` | integer |  |
| `results[].packs[].path` | string |  |
| `results[].packs[].release` | string |  |
| `results[].packs[].state` | string | One of `installed`, `update`, `available`. |
| `results[].packs[].licence` | string |  |
| `results[].packs[].parts` | array of object |  |

### `install` [#install]

**Install packs.** Downloads packs from the catalog into the store, verified, and serves them: `packs` as catalog paths or addresses (np, np.places, ae-du), `components` (every one when none). The person confirms the download's size when the client can ask; otherwise the call is refused past `max_bytes` (default 100 MB), naming the size. Answers a task, or a `handle` for `install_status`.

Listed only by a server over a store that allows installs (`--allow-install`).

Hints: changes the store, idempotent, reaches the network.

| Argument | Type | Required | What |
|---|---|---|---|
| `packs` | array of string | yes | 1 to 16 items. |
| `components` | array of string |  |  |
| `max_bytes` | integer |  | The most this call may download without the person's confirmation; default 100,000,000. At least 0. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | null |  |
| `result` | object |  |
| `result.status` | string | One of `working`, `completed`, `failed`, `cancelled`, `declined`. |
| `result.download` | integer |  |
| `result.error` | object or null |  |
| `result.handle` | string |  |
| `result.message` | string |  |
| `result.packs` | array of string |  |
| `result.result` | object or null |  |

### `install_status` [#install_status]

**How an install goes.** Where the install of a `handle` (from `install`) is: working, completed, failed or cancelled, with its message and, once ended, what it installed or why it failed. Ask about once a second.

Listed only by a server over a store that allows installs (`--allow-install`).

Hints: read-only, idempotent.

| Argument | Type | Required | What |
|---|---|---|---|
| `handle` | string | yes | 32 to 32 bytes. |

Answers:

| Field | Type | What |
|---|---|---|
| `attribution` | null |  |
| `result` | object |  |
| `result.status` | string | One of `working`, `completed`, `failed`, `cancelled`, `declined`. |
| `result.download` | integer |  |
| `result.error` | object or null |  |
| `result.handle` | string |  |
| `result.message` | string |  |
| `result.packs` | array of string |  |
| `result.result` | object or null |  |

## Places [#places]

The answer of every place tool: a page of `results`, the `next_cursor` of the next page when there is one, and the `attribution` of the packs that answered. A field a place lacks is left out or null.

| Field | Type | What |
|---|---|---|
| `attribution` | string or null | The credits of the packs that answered: show them wherever the data is shown. |
| `results` | array of object |  |
| `results[].admin_level` | integer | 1 for a country's first-level units, deeper units after; 0 for a country and for a place that is not an admin unit. |
| `results[].elevation` | integer or null | Metres above EGM2008. |
| `results[].id` | string | Its stable ID, such as tgd:np/1xhb. |
| `results[].kind` | string or null | Its kind, such as settlement or admin. |
| `results[].name` | string or null | Its canonical name. |
| `results[].parent` | string or null | Its parent's ID; null for a root. |
| `results[].timezone` | string or null | Its IANA timezone, such as Asia/Kathmandu. |
| `results[].bearing` | number | Degrees clockwise from north, from the place to the point asked about. |
| `results[].elevation_source` | string | Where the height came from: surveyed or model, when stated. |
| `results[].inside` | boolean | Whether the point asked about is inside the place's outline (reverse). |
| `results[].is_dst` | boolean | Whether daylight saving time is in force at that instant. |
| `results[].iso_code` | string | Its ISO 3166 code, for a country or unit that has one. |
| `results[].lat` | number | Latitude, WGS84 degrees. |
| `results[].lon` | number | Longitude, WGS84 degrees. |
| `results[].meters` | number | Distance from the point or place asked about, metres. |
| `results[].names` | array of object | Every name, canonical first (get_place with names). |
| `results[].names[].machine` | boolean | Made by a machine (a romanisation, a transliteration or a donor label): an app may show it differently. |
| `results[].names[].origin` | string | Where the name came from. |
| `results[].names[].text` | string |  |
| `results[].names[].kind` | string | The name's type. |
| `results[].names[].lang` | string | A BCP 47 tag, such as ne-Deva. |
| `results[].population` | integer | Its number of people, when known. |
| `results[].population_kind` | string | Where the number came from: stated (a source's count) or modelled (a population grid over an admin unit). |
| `results[].score` | integer | Its search score, comparable only within one query. |
| `results[].tz_abbr` | string | The zone's abbreviation at that instant. |
| `results[].utc_offset` | string | The UTC offset at the instant asked, such as +05:45 (timezone). |
| `next_cursor` | string | Pass as `cursor` to the same tool for the next page; absent on the last. |

## Resources [#resources]

| URI template | What | Type |
|---|---|---|
| `teigeodata://place/{id}` | A place: A place by its ID (tgd:np/1xhb), with every name, as a GeoJSON feature | `application/geo+json` |
| `teigeodata://boundary/{id}{?lod}` | A place's outline: A place's outline as a GeoJSON feature, simplified to level of detail `lod` (0 finest to 6 coarsest) | `application/geo+json` |
| `teigeodata://dataset/{address}/{component}` | A pack: A pack (np/places): its release, places, installed components and attribution | `application/json` |

## Errors [#errors]

A tool that fails answers a tool error (`isError`) whose text is `{"error": {"code": "...", "message": "..."}}`, with the codes of the [HTTP API](/docs/reference/http#errors).