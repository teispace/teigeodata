---
title: "Nominatim compatibility: the routes, parameters and fields clients use"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-02-15
---

# Nominatim compatibility

## 1. Question

What the server's Nominatim-compatible `/search`, `/reverse` and `/lookup` (ADR-0038, phase
6.2) must accept and answer, so that an app switches to teigeodata by changing a URL: which
parameters and formats Nominatim 5.3 defines, which of them real clients send, and which
fields they read. Feeds the routes and their conformance corpus.

## 2. Summary and recommendation

- **Routes** `/search`, `/reverse` and `/lookup` at the root (D-312 left them free), formats
  `json` and `jsonv2` (the same for us: we have neither OSM `class`/`category` nor
  `place_rank`) and `geojson`. The default is `jsonv2`, as Nominatim's own default for an
  installation; `/reverse`'s documented default `xml`, and `xml` and `geocodejson` anywhere, are
  refused with 400 rather than answered in another format.
- **`/search`:** `q`, or the structured fields (`amenity`, `street`, `city`, `county`, `state`,
  `country`, `postalcode`) joined into one query; `limit` (default 10, at most 40, as
  Nominatim); `countrycodes` (a filter on the ISO code of the place's country); `viewbox` (a
  focus on the box's centre, scaled to its half-diagonal) and `bounded=1` (a filter to the
  box); `addressdetails`. Nothing found is `[]`.
- **`/reverse`:** `lat`, `lon`, `zoom` (0–18, default 18), `addressdetails` (default 1). Below
  zoom 10 it answers the deepest admin unit holding the point within a depth set by the zoom
  (3: the country, 5: depth 1, 9: depth 2); from 10 the nearest place in the point's country.
  No answer is `{"error": "Unable to geocode"}` with 200, which geopy reads as "no result".
- **`/lookup`:** `osm_ids`, up to 50. Our places have no OSM IDs to look up by, so an entry
  that is one of our IDs (`tgd:…`, the `place_id` we answer) is looked up and an OSM reference
  (`N123`) is left out, as a place not found is.
- **Fields:** `place_id` (our stable ID, not Nominatim's internal number), `licence` (the
  attribution of the packs that answered, D-032), `lat`, `lon` and `boundingbox` as strings,
  `name`, `display_name`, and `address` (`country`, `admin_level_<n>`, `country_code`) when
  asked. Left out, as we cannot fill them: `osm_type`, `osm_id`, `class`/`category`, `type`,
  `place_rank`, `importance`, `icon`, `extratags`, `namedetails`, geometries.
- **Other parameters** (`accept-language`, `extratags`, `namedetails`, `polygon_*`, `email`,
  `dedupe`, `layer`, `featureType`, `debug`) are ignored: each only adds detail we leave out.
  `json_callback` (JSONP) is refused: honouring it runs a caller-named script, and ignoring it
  breaks the client silently.
- **Errors** are Nominatim's shape, `{"error": {"code": 400, "message": "…"}}`, with the HTTP
  status.

## 3. Findings

**Nominatim 5.3.3 (2026-10-14, the latest release, checked 2026-11-15)**, its API docs
(`docs/api/Search.md`, `Reverse.md`, `Lookup.md`, `Output.md`):

- `/search`: free-form `q` or structured fields; `format` one of `xml`, `json`, `jsonv2`,
  `geojson`, `geocodejson`, default `jsonv2` (the public service forwards a request without
  `format` to its web UI); `limit` default 10, "cannot be more than 40"; `countrycodes` a hard
  filter of ISO 3166-1 alpha-2 codes; `viewbox` `x1,y1,x2,y2` (x is longitude) a boost, a
  filter with `bounded=1`; `addressdetails` default 0.
- `/reverse`: one result, or an error where there is no data; `format` default `xml`;
  `addressdetails` default 1; `zoom` 0–18 (3 country, 5 state, 8 county, 10 city, 12 town,
  13 village, 14 neighbourhood, 15 any settlement, 16–18 streets and buildings).
- `/lookup`: `osm_ids` mandatory, each `N`, `W` or `R` and a number, up to 50.
- JSON: an array of places (search, lookup) or one place (reverse); `lat`, `lon` and
  `boundingbox` are strings; `jsonv2` renames `class` to `category` and adds `place_rank`;
  GeoJSON features carry the same properties with a `bbox`.

**geopy** (`geopy/geocoders/nominatim.py`, master, read 2026-11-15): sends `format=json`, `q` or
the structured fields, `limit`, `viewbox` with `bounded`, `countrycodes`, `addressdetails`,
`namedetails`, `accept-language`, `extratags`, `polygon_*`, `featureType`; reverse sends `lat`,
`lon`, `format=json`, `addressdetails`, `zoom`. It reads `lat`, `lon` and `display_name` (and
keeps the rest as `raw`); a response object with `error` equal to `Unable to geocode` is no
result, any other `error` raises.

**Leaflet Control Geocoder** (`src/geocoders/nominatim.ts`, master, read 2026-11-15): search sends
`q`, `limit=5`, `format=json`, `addressdetails=1`; reverse sends `lat`, `lon`, `zoom` (from the
map scale), `addressdetails=1`, `format=json`. It reads `boundingbox` (four strings), `lat` and
`lon` (converted with `+`), `display_name`, and `address` (`road`, `building`, `city`, `town`,
`village`, `hamlet`, `state`, `country`) for its label; a reverse answer without `lat` is no
result.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Map our native answer to Nominatim's shape, leave out what we cannot fill | Honest (ADR-0038); clients read what they need | Clients that need OSM IDs get none | n/a | n/a | **Chosen** |
| Fill `class`/`type`/`city`/`state` by guessing from our kinds and admin depth | Richer labels in Leaflet | Fakes OSM tags; admin depth means different things per country (P16) | n/a | n/a | Rejected |
| Answer `/reverse` as XML by default, as documented | Exact default | An XML writer for one default no client we found relies on | n/a | n/a | Rejected: 400 names the formats |

## 5. What we build ourselves, and why

The routes over the request core (D-311): the parameters parse into the same `Ask`s as the
native routes, so the bounds, the seams, the panic guard and the fuzzing cover them. The
conformance corpus (`crates/teigeodata-server/tests/nominatim.json`) lists each client's
requests and the fields it reads; the test runs every one against the server and checks each
field's presence and type, and the errors a client tells apart.

## 6. Impact on the plan

- Phase 6.2's Nominatim item; D-314.
- `servers.md` §3 said `importance` would carry our score: it is left out instead, since our
  score is not on Nominatim's 0–1 scale and a client may compare them.

## 7. Risks and open items

- Only two clients are in the corpus; others (Photon clients, mobile SDKs) are added as they
  are met.
- A client that keeps OSM IDs from an earlier Nominatim answer finds nothing by `/lookup`.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Nominatim | 5.3.3 (2026-10-14) | 2026-11-15 | GitHub releases API |
| geopy | master | 2026-11-15 | GitHub, `geopy/geocoders/nominatim.py` |
| Leaflet Control Geocoder | master | 2026-11-15 | GitHub, `src/geocoders/nominatim.ts` |

## 9. Sources

- [Nominatim API: Search](https://nominatim.org/release-docs/latest/api/Search/), Reverse,
  Lookup and Output, from `osm-search/Nominatim` `docs/api/`, accessed 2026-11-15.
- [geopy Nominatim geocoder](https://github.com/geopy/geopy/blob/master/geopy/geocoders/nominatim.py),
  accessed 2026-11-15.
- [Leaflet Control Geocoder, Nominatim](https://github.com/perliedman/leaflet-control-geocoder/blob/master/src/geocoders/nominatim.ts),
  accessed 2026-11-15.
- [ADR-0038](../../03-decisions/adr/ADR-0038-compatibility-outputs.md),
  [servers](servers.md).
