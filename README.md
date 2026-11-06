# teigeodata

**Accurate, offline place data for developers.** Coordinates, elevation, the full admin
hierarchy down to the lowest level, boundaries, local names, exact IANA timezones, postal codes
and population, cleaned and deduplicated from the best open sources. It ships as small, signed
packs that you bundle at build time or download at runtime, with one API in every major language.

**The open, offline gazetteer you are allowed to keep.** Commercial geocoding APIs limit how
long you may store their results; teigeodata's data is yours to store, ship and redistribute
under its licence.

A [Teispace](https://github.com/teispace) project. Code under Apache-2.0; data under ODbL, with
a CC-BY 4.0 lite build.

> **Status:** Phase 0 (foundations and research). Nothing is published yet. See
> [progress](docs/08-progress/progress.md) and the [roadmap](docs/07-planning/roadmap.md).

## What it will do

- **Search** across scripts and spellings: काठमाडौं, Kathmandu, Kathamandu and Kantipur find the
  same place; typos and abbreviations too.
- **Reverse geocode** a point to ward, municipality, district, province and country.
- **Timezone** at any point, exactly as the IANA boundaries draw it, with offsets and
  transitions.
- **Elevation** for every place, and at any point with an elevation pack.
- **Boundaries** at several levels of detail, ready for MapLibre, Google Maps or flutter_map.
- **Postal codes**, current and former, in both directions.
- **Offline first:** Nepal in a few MB inside your app; your users download Dubai when they need
  it; packs merge live, update by delta, and are verified.
- **Same API everywhere:** Rust, C, C++, Node, WASM, Dart and Flutter, Python, Java and Kotlin,
  Android, Swift, .NET, Go, plus an MCP server and a CLI.
- **Tree-shakable:** pay only for the capabilities and data you use.

Coverage starts with Nepal, then India, South Asia, Asia, the USA and the rest of the world.

## Documentation

| If you want to know | Read |
|---|---|
| Why and what | [vision](docs/00-vision/vision.md), [scope](docs/00-vision/scope.md) |
| How it is built | [architecture](docs/04-architecture/overview.md), [decisions](docs/03-decisions/log.md) |
| How to contribute | [CONTRIBUTING.md](CONTRIBUTING.md), [GOVERNANCE.md](GOVERNANCE.md) |
| Everything | [docs map](docs/README.md) |

## Licence

Code: [Apache-2.0](LICENSE). Data: ODbL 1.0 (main) and CC-BY 4.0 (lite), with attribution in
`ATTRIBUTION.md` and in every pack. See [NOTICE](NOTICE) and [CLEAN_ROOM.md](CLEAN_ROOM.md).
