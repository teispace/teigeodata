# teigeodata

**Accurate, offline place data for developers.** Coordinates, elevation, the full admin
hierarchy down to the lowest level, boundaries, local names, exact IANA timezones, postal codes
and population, cleaned and deduplicated from the best open sources. It ships as small packs,
every section hashed and checked as it is read, that you bundle at build time or download at
run time, with one API in every major language.

**The open, offline gazetteer you are allowed to keep.** Commercial geocoding APIs limit how
long you may store their results; teigeodata's data is yours to store, ship and redistribute
under its licence.

A [Teispace](https://github.com/teispace) project. Code under Apache-2.0; data under ODbL, with
a CC-BY 4.0 lite build. **Docs, guides and a playground:**
[teigeodata.teispace.com](https://teigeodata.teispace.com).

> **Status:** 1.1 is released on every package registry ([CHANGELOG.md](CHANGELOG.md)). Nepal
> is covered in full, with the world's countries, first-level units and cities of 5,000 or
> more, and India's districts and towns; coverage grows by data releases, India next. Data
> packs are GitHub pre-releases until the signed catalog goes live. See
> [progress](docs/08-progress/progress.md) and the [roadmap](docs/07-planning/roadmap.md).

## Install

| Language | Package |
|---|---|
| Rust | `cargo add teigeodata-engine` |
| Python | `pip install teigeodata` |
| Node.js | `npm install teigeodata` |
| Browser (WebAssembly) | `npm install teigeodata-wasm` (or `teigeodata-wasm-lookup`, without name search) |
| Dart and Flutter | `dart pub add teigeodata` |
| Kotlin, Java, Android | `com.teispace:teigeodata`, `com.teispace:teigeodata-android` on Maven Central |
| Swift | Swift Package Manager, `https://github.com/teispace/teigeodata` |
| Go | `go get github.com/teispace/teigeodata/bindings/go` |
| C, C++ | the release archives `teigeodata-c-<version>-<target>` (pkg-config and CMake) |
| .NET | from a checkout; not on NuGet yet |
| Command line (`tgd`) | `cargo install --locked teigeodata-cli` |

Then [get a pack and ask it something](https://teigeodata.teispace.com/docs/getting-started).

## What it does

- **Search** across scripts and spellings: काठमाडौं, Kathmandu and Kathamandu find the same
  place; "Name, Parent" ranks the place in that parent first; a focus point ranks what is near.
- **Reverse geocode** a point to the nearest places, and to every admin unit holding it, from
  the country down to the ward.
- **The hierarchy**: every unit with its ISO 3166 code, parents and children, and stable IDs that
  survive data updates.
- **Boundaries** as GeoJSON at the level of detail you ask for.
- **Timezones** at any point, exactly as the IANA boundaries draw it, with offsets, transitions
  and skipped or repeated local times from the tzdb rules in the pack.
- **Elevation** for every place, and at any point with an elevation pack.
- **Postal codes**: look up, autocomplete, a unit's codes, the code at a point.
- **Nearby and distance**: places within a radius or nearest a place; distance and bearing.
- **Local names** in each language and script, with reviewed corrections, and **population**
  with its source.
- **Offline first:** bundle a pack, or let the store download, verify and keep the countries a
  user picks.
- **Same API everywhere:** Rust, C, C++, Swift, Kotlin, Java, Dart and Flutter, Python, Node.js,
  WebAssembly, Go and .NET, plus a CLI, an HTTP server (with Nominatim-compatible routes) and
  an MCP server for AI agents.
- **Tree-shakable:** pay only for the components and data you use.

Coverage grows from Nepal to India, South Asia, Asia, the USA and the rest of the world.

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
