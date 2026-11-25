# Replace a Nominatim server (/docs/tutorials/replace-nominatim)

Clients written for Nominatim (geocoder plugins, mobile SDKs, scripts) call `/search`,
`/reverse` and `/lookup`. `tgd serve` answers those routes from a pack, alongside its own JSON
and GeoJSON API, with no database to import and no rate limit but your own.

<Steps>
<Step>
### Install the command line [#install-the-command-line]

```sh
cargo install --locked teigeodata-cli
```
</Step>

<Step>
### Get a pack and serve it [#get-a-pack-and-serve-it]

```sh
curl -LO https://github.com/teispace/teigeodata/releases/download/data-2026.11.14.3/world-np-lite.tgd
tgd serve world-np-lite.tgd
```

The server binds to loopback by default; put it behind your reverse proxy to expose it.
</Step>

<Step>
### Point the client at it [#point-the-client-at-it]

Clients set Nominatim's base URL to the server's. A search, as a client sends it:

```sh title="tour.sh"
curl -s "$URL/search?q=Pokhara&format=jsonv2&limit=1"
```

```json title="Response"
[{"boundingbox":["28.26689","28.26689","83.968","83.968"],"display_name":"Pokhara, Pokhara, Kaski, Gandaki, Nepal","lat":"28.26689","licence":"Made with Natural Earth (https://www.naturalearthdata.com), public domain; GeoNames (https://www.geonames.org), CC BY 4.0; Time zones: IANA Time Zone Database 2026f (public domain); Wikidata (https://www.wikidata.org), CC0 1.0; Unicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3; Local Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in); Administrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0; Elevation: NASADEM, NASA JPL and LP DAAC; Buildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/); Buildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0; Central Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution","lon":"83.968","name":"Pokhara","place_id":"tgd:np/1xhb"}]
```
</Step>

<Step>
### Check what it holds [#check-what-it-holds]

`/search` takes `q` or the structured fields, `limit` (at most 40, as Nominatim),
`countrycodes`, `viewbox` and `bounded`; `/reverse` takes `lat`, `lon` and `zoom`. The
[HTTP server page](/docs/servers/http#nominatim-compatible) lists every parameter and
what differs from Nominatim.
</Step>
</Steps>

Measured on loopback, a pack answers tens of thousands of Nominatim searches a second with a
peak footprint of about 16 MB ([server load](/docs/data/measured#server-load)).