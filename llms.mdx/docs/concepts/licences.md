# Licences (/docs/concepts/licences)

## The code [#the-code]

All code (the engine, the bindings, the command line, the servers, the forge) is under the
[Apache License 2.0](https://github.com/teispace/teigeodata/blob/main/LICENSE): use it in open
and closed software alike, keep the notices.

## The data: two builds [#the-data-two-builds]

Every pack comes in one of two builds, built from the same schema with the same IDs:

| | Main build | Lite build |
|---|---|---|
| Licence | ODbL 1.0 | CC BY 4.0 |
| Sources | Open sources, OpenStreetMap included (no other share-alike, nothing non-commercial) | Only public-domain and attribution-only sources (clean room) |
| You must | Credit the sources; share improvements to the database you distribute | Credit the sources |
| Best for | The most complete data | Apps that cannot take on share-alike terms |

The lite build is **clean room**: its pipeline refuses share-alike, non-commercial and
proprietary sources at ingest, before any step reads them, and a licence gate fails a build
whose provenance includes one. The rules are in
[CLEAN_ROOM.md](https://github.com/teispace/teigeodata/blob/main/CLEAN_ROOM.md).

## You may keep it [#you-may-keep-it]

Commercial geocoding APIs limit how long you may store their answers. teigeodata's data is yours
to store, cache, ship inside apps and redistribute, under the build's licence. There are no
per-request terms and no keys.

## Attribution [#attribution]

Every pack carries the notices of its sources, and every answer can show them:

- `tgd inspect` lists a pack's notices, and `/packs` on the HTTP server returns them.
- Every HTTP and MCP answer carries an `attribution` (the `licence` field in Nominatim's
  format), built from the packs that answered.
- In an app, show the notices where you show the data, as you would map credits: an "About" or
  "Data sources" screen is usual. The world lite pack's notices today:

```text
Made with Natural Earth (https://www.naturalearthdata.com), public domain
GeoNames (https://www.geonames.org), CC BY 4.0
Time zones: IANA Time Zone Database 2026f (public domain)
Wikidata (https://www.wikidata.org), CC0 1.0
Unicode CLDR (https://cldr.unicode.org), Copyright © 1991-2026 Unicode, Inc., under the Unicode License v3
Local Government Directory, Ministry of Panchayati Raj, Government of India (https://lgdirectory.gov.in)
Administrative boundaries: Survey Department of Nepal and UN Resident Coordinator's Office in Nepal, via OCHA HDX (cod-ab-npl), CC BY-IGO 3.0
Elevation: NASADEM, NASA JPL and LP DAAC
Buildings: Microsoft Global ML Building Footprints, CDLA-Permissive-2.0 (https://cdla.dev/permissive-2-0/)
Buildings: Google Open Buildings v3 (Sirko et al. 2021, arXiv:2107.12283), CC BY 4.0
Central Bureau of Statistics (now National Statistics Office), Nepal, via Open Data Nepal (https://opendatanepal.com), Creative Commons Attribution
```

Credit the build's licence with them: each pack states it (`licence: "cc-by-4.0"` in
`tgd inspect` for a lite pack).