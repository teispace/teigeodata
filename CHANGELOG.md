# Changelog

Every change to the crates, by version. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/) with its 0.x rules: before 1.0.0 a minor release may break the
public API, and every break is listed under **Breaking**. How releases are cut:
[docs/06-engineering/releases.md](docs/06-engineering/releases.md).

The stable surface is the public Rust API of `teigeodata-engine` and `teigeodata-sync`, the
`tgd` command line and its JSON output.

## [Unreleased]

### Added

- The Dart package runs on the web, compiled by dart2js or dart2wasm, over the WebAssembly
  module, with the classes it has natively (D-307). `TeiGeodata.init()` loads the module once
  on the web and returns at once natively; the web opens packs from their bytes.
- `Engine::open_file_range(path, offset, length)` (and `_with`) in every language but
  WebAssembly: opens a pack stored whole inside another file, such as an uncompressed asset of
  an Android APK (`noCompress`), mapped in place (D-306).
- Releases are built, attested and published by one workflow, `release.yml` (D-302): every
  file with build provenance through Sigstore, a CycloneDX SBOM per crate and `SHA256SUMS`;
  `gh attestation verify <file> --repo teispace/teigeodata` checks one.
- The crates `teigeodata-core`, `-blob`, `-format`, `-names`, `-sync`, `-engine`, `-api`,
  `-server` and `-cli` are publishable to crates.io (D-303, D-310).
- `teigeodata-server`, the request core both servers and `tgd` share (D-310, D-311): the
  native, GeoJSON and Nominatim-compatible outputs moved there from the CLI; `query`, every
  place, elevation and postal query with its bounds (`Limits::SERVER`: 256 bytes of text, 50
  results, 1,000 km) and stable error codes (`Code`, each with its HTTP status); `packs`, the
  pack set over files and the store, following the store's journal while it answers; and
  `seams`, who is asking, may they and how much they used, open by default (ADR-0025).
- `tgd serve` (feature `serve`, on by default) and `teigeodata_server::http` (feature `http`):
  every query as a `GET` route under `/v1/`, `/packs` and `/health`, on axum; bound to
  127.0.0.1, CORS off unless configured, a 4 KiB query string, a request timeout, a panic
  answered `internal`; follows the store's journal every 5 s (D-312).
- `tgd serve` answers Nominatim's `/search`, `/reverse` and `/lookup` (`json`, `jsonv2`,
  `geojson`) with the parameters geopy and Leaflet Control Geocoder send, so an app switches
  by changing its URL; fields we cannot fill are left out (D-314).
- `Store::read_packs`: the packs the journal lists, read without opening the store, so a reader
  beside an install never recovers or removes its files (D-311).
- Each release carries the C library alone for every target the Dart hook and the Go loader
  download, Android and iOS included, its SHA-256 pinned in both (D-304); the Dart package is
  publishable to pub.dev, and the JVM binding is bundled for Maven Central as
  `com.teispace:teigeodata` and `com.teispace:teigeodata-android`.
- The parity gate (D-300): `cargo xtask parity-mutants` plants each value of
  `bindings/parity.json` and requires every language's parity suite to fail on it; C and C++
  read the values from the generated `bindings/c/parity_data.h`. The suites check the values
  the gate found unchecked (column names, `get`'s longitude, `dst`, error codes).
- `bindings/bench.sh` measures a 1,000-row query decoded in every language (D-301).
- The .NET binding, NuGet package `Teispace.TeiGeodata` (`net10.0`, D-298): the whole API as
  source-generated, blittable P/Invoke (NativeAOT and trimming safe), `SafeHandle` handles,
  `…Async` forms of the blocking store calls, `Job.ProgressAsync` and a cancellable
  `ResultAsync`, `BlobReader` for blobs and `TeiGeodataException` with the stable codes.
- The Go binding, `github.com/teispace/teigeodata/bindings/go` (Go 1.26+, D-297): the whole
  API through cgo, or through purego with `CGO_ENABLED=0`, over the C library loaded at run
  time (`LoadLibrary`, `TEIGEODATA_LIB`, a pinned prebuilt in the user cache filled by
  `fetch`, or the system path); `Reader` for blobs; `*Error` with the stable codes.
- `teigeodata.hpp`, a header-only C++17 wrapper of the C library, in the C package
  (D-295): owning handles, `Blob`, exceptions with the stable error codes.
- The C package for Linux musl (x64 and arm64).
- The Node.js packages: `@teispace/teigeodata` with a prebuilt addon per platform
  (`@teispace/teigeodata-<platform>`, macOS, Linux glibc and musl, Windows, x64 and arm64), npm
  installing only the one a machine needs; CommonJS and ES module entries; Node.js 22 and later;
  assembled by `cargo xtask package-node` (D-294).
- The WebAssembly packages `@teispace/teigeodata-wasm` (every component) and
  `@teispace/teigeodata-wasm-lookup` (no search): one ES module for browsers, Deno, Bun,
  Node.js and Workers, the `Engine` over pack bytes, built and size-checked by
  `cargo xtask package-wasm` (D-293).
- `components()` in every binding: the components the build answers. `teigeodata-api` has a
  Cargo feature per pack component, all in `components` (default); a build without one keeps
  every function and answers as a layer installed without it (D-291).
- `teigeodata-blob` format v1 (D-275, spike S2): a null bitmap per column
  (`Column::Nullable`, `Reader::nulls`), dictionary-encoded strings (`Column::Dict`, read as
  `View::Dict`: each distinct string stored once), and `u8`, `i16`, `u16` columns. v0 blobs
  still read.
- The `Engine` handle in every binding (C, Python, Node, JVM, Swift, Dart): open packs from a
  file or bytes (each answers its layer ID; `remove` takes it), then `get`, `search`, `reverse`, `admin_at`, `parent`, `ancestors`,
  `children`, `nearby`, `within_radius`, `tz_at`, `by_iso`, `elevation_at`, and postal codes
  (`postal_lookup`, `postal_prefix`, `postal_codes_of`, `postal_at`), each answering a blob. One
  engine is safe from many threads at once (D-274).
- Error codes `Io` (4) and `Corrupt` (5).
- IDL format 2: handles with methods, `&[u8]` parameters, handle parameters and results.
- IDL format 3: options records (D-276), plain structs of scalar and optional scalar fields
  whose defaults are their zero values. `SearchOptions` (a focus point and scale, `strict`)
  and `Engine::search_with` use them in every binding; leaving the options out (C `NULL`)
  means the defaults.
- IDL format 4: enums (D-277), each its language's own enum. `OpenOptions` (priority,
  `Verify` lazy/eager/trusted, `Load` map/read, outline level and tolerance) with
  `Engine::open_file_with` and `open_bytes_with` in every binding.
- IDL format 5: text fields in records (`String`, empty by default). `OpenOptions` gains
  `components` (`point,names,search`; what they need is added) and `languages` (`en,ne`, at
  most 8), for packs installed in part or read in some languages only.
- Error code `MissingComponent` (6, D-278): a query needs a component the pack was opened
  without; before, this was `InvalidArgument`.
- The `Store` and `Job` handles in every binding (D-279, research note store-api): `create`,
  `open`, `catalog`, `status`, `pin`, `remove`, `engine` (an engine over the installed packs),
  `find` and `locate` (the catalog's packs for a name or a point, over any engine, offline),
  and `refresh`, `install` (with `InstallOptions`: components, languages, outline, pin),
  `update` and `repair` as jobs on a thread of their own, with `wait_progress`, `seq`,
  `done_bytes`, `total_bytes`, `is_ended`, `cancel` and `result`. A cancelled install leaves
  the store as it was.
- Error codes `Network` (7) and `Refused` (8) (D-279).
- Node: every call that blocks (the store's, a job's `waitProgress` and `result`) has a
  `…Async` form returning a `Promise` (D-280); the IDL marks such calls `blocks` from a
  `# Blocks` documentation section.
- `Store::install_pass` in every language: one pass of an install whose bytes a platform's
  download service fetches; its result lists each missing range and the file to save it as
  (D-282).
- An install whose catalog refresh fails, when the stored catalog lacks the pack, fails with
  `Network` (the cause) instead of `Refused` (D-282).
- IDL: an options record must be the last parameter, so every language can default it.
- Python wheels (D-290): `bindings/python/pyproject.toml` builds with maturin; cibuildwheel
  makes a cp311-abi3 wheel (CPython 3.11 and later), a cp315-abi3t wheel (3.15 and later,
  free-threaded included) and a cp314t wheel per platform (`python.yml`). A threads test
  checks one engine from many threads and that the GIL stays off.
- The Dart package's build hook (D-289): Dart and Flutter apps get the native library with no
  manual step, built from a checkout (`hooks.user_defines.teigeodata.source`, or in the
  repository) or downloaded from the release and checked against its pinned SHA-256; a given
  file (`library:`) overrides both. `cargo xtask flutter-check` runs the example app's
  integration test on the desktop, an iOS simulator or a device; `flutter.yml` covers macOS,
  iOS, Linux, Windows and Android.
- The JVM binding's packages (D-288): `cargo xtask package-jvm` makes the Android AAR (arm64-v8a,
  armeabi-v7a, x86_64; minSdk 24; 64-bit libraries checked for 16 KB pages) and the desktop JAR,
  whose `NativeLoader` extracts its platform's library when `java.library.path` has none;
  parity runs from the JAR and on an Android emulator; `jvm.yml` builds six desktop platforms.
- The Swift package's XCFramework (D-287): `cargo xtask package-swift [--test]` builds a
  static `CTeiGeodata.xcframework` (iOS, the iOS simulators, macOS) and tests the Swift
  binding against it on macOS and an iOS simulator; a privacy manifest; `Package.swift` gains
  an XCFramework mode (`TEIGEODATA_XCFRAMEWORK=1`) and the parity checks a Swift Testing test.
- The C library as it ships (D-286): `libteigeodata` (crate `teigeodata-c`) with an
  `@rpath` install name and a versioned soname, `cargo xtask package-c` (header, shared and
  static libraries, pkg-config, CMake, smoke-tested), full and lite archives for six targets
  from `c-package.yml`; the header guards the store's declarations with
  `TEIGEODATA_FEATURE_STORE`.
- `cargo xtask abi` (D-285): the C ABI's compatibility with the last release and the symbols
  the built library exports, on the full and the lite library in CI.
- The zone rules in every language (D-284): `Engine::tz_offset_at` (offset, DST flag and
  abbreviation at an instant), `tz_local` (a civil time's offsets: unique, a gap or an
  overlap), `tz_zones`, `tz_rules_start` and `tz_version`; parity on a rules-only fixture.
- A job's idioms in each language, from the IDL's `# Job` contract (IDL format 7): a
  progress stream (Swift `progress`, JVM `onProgress`, Node `progress()`, Dart `progress()`,
  Python `teigeodata.aio.progress`), and cancel through the language's own cancellation
  (Swift task cancellation, JVM `future.cancel`, Node `AbortSignal`, Python task
  cancellation via `teigeodata.aio.result`) (D-283).
- Swift, the JVM and Dart: the same `…Async` forms, as an `async throws` function, a
  `CompletableFuture` and a `Future` run on a helper isolate; in Dart a handle closed during
  one is freed when it ends. Python awaits them with `asyncio.to_thread` (D-281).
- `teigeodata_sync::fetch::Ureq::with_timeouts`, and a 30 s timeout for a response's head:
  a host that accepts and never answers no longer holds a download forever (D-280).
- IDL format 6: a handle, record or function may carry `#[cfg(feature = "…")]`; every
  generated Rust layer gates it the same way. The `store` feature (on by default) of
  `teigeodata-api`, `teigeodata-ffi` and the Python, Node and JVM crates holds the store;
  without it the C library is 949 KB instead of 3.1 MB (D-279).

### Changed

- `tgd`'s GeoJSON `attribution` and Nominatim `licence` name the packs the places came from,
  not every pack given, and are absent when nothing was found (D-311). `tgd near --radius`
  refuses a radius that is not positive and finite.
- A cancel stops a metadata download at its next chunk, and its bytes count as progress:
  `Transport::fetch_streamed` (the `http` transport reads in 16 KiB chunks); before, a slow
  host held a refresh until its body ended (D-283).
- The C library's Apple builds leave header room for a rewritten install name
  (`-headerpad_max_install_names`), as Dart, Flutter and other packagers rewrite it (D-289).
- The JVM binding keeps to Android API 24: handles are freed by a hand-written
  `NativeCleaner` instead of `java.lang.ref.Cleaner` (Android API 33), so the classes load on
  older devices; `NativeLoader` uses `java.io` (`java.nio.file` is API 26); parity runs on API
  24 and API 35 emulators (D-288).
- Dart: calls that block are no longer leaf calls; a leaf call blocking in a helper isolate
  kept every isolate of the group from a safepoint and stalled them (D-283).

- The `http` transport verifies TLS with Mozilla's roots (`webpki-roots`), as it always did:
  the unused `rustls-platform-verifier` is dropped (D-279 amends D-168).

- The store's policy (init, refresh, install with dependencies and selections, update,
  remove, pin, repair, status) moved from the command line into `teigeodata_sync::client`
  (feature `client`), for the bindings to share; `tgd` formats its results. `tgd install`
  now prints its lines when the whole install ends.
- `teigeodata_sync::watch`: a `Watch` (progress in bytes, a sequence to wait on, cancel) and
  `Watched`, a transport that reports to it and stops at the next request or chunk once
  cancelled (`FetchError::Cancelled`, `ClientError::Cancelled`). `Transport::expect` passes
  the catalog's size of the work to come.

### Fixed

- `Shared::follow` (and `Engine::follow`) opens a store pack with the parts it was installed
  with (D-251); before, a pack of some parts was opened as whole, its parts not installed read
  as holes (D-311).

### Breaking

- `fold` can fail: in a build without the `search` component it throws (returns)
  `MissingComponent`, as its Unicode tables belong to search (D-292). Swift calls it with
  `try`; the other languages throw unchecked, as from any other fallible call.
- Python: blob results are `teigeodata.Blob`, lent without a copy through the buffer
  protocol, not `bytes` (`bytes(blob)` copies); the reader `teigeodata.blob.Blob` is now
  `teigeodata.blob.Reader` and reads any buffer; CPython 3.11 is the oldest supported (3.10
  is past its end of life) (D-290).
- Dart: the bindings are `@Native` functions of the package's code asset (D-289).
  `TeiGeodata.open(path)` is gone; use `TeiGeodata()`. The SDK floor is Dart 3.10.
- Blobs start with the magic `TGB1` instead of `TGB0`. A binding or caller that checks the
  magic compares with `TGB1` (or accepts both); the layout of the v0 types is unchanged.

## [0.3.1] - 2026-11-14

Search ranks countries, regions and cities on one scale and weighs short substring matches
less; the forge's world parts gain aliases, unit populations and ISO 3166 codes. No public
API changed, and the pack format is unchanged: every data release reads here.

### Added

- `forge world --alternates <source>`: each city's GeoNames alternate names in the part's
  languages, historical and colloquial ones included (Bombay, Madras, Benares; D-265).

- `forge world` gives each unit its population from a pinned Wikidata snapshot
  (`wikidata-unit-population`; `--no-populations` leaves it out), and places a city outside
  every unit's outline in the nearest unit of its country within 10 km (D-266).

- `forge world --aliases <source>` (profile `world_aliases`): every country, unit and city
  takes its Wikidata aliases, so Poona, Simla, Tanjore, Peking and Ceylon find their place
  (D-268).

- Every world part carries ISO 3166 codes: 237 countries and 4,071 of 4,567 first-level
  units, current codes only (India's `IN-OD`, `IN-UK`, `IN-CG`, `IN-TS`), so `iso_code` and
  `by_iso` answer world-wide (D-271).

### Changed

- A substring match of 3 or 4 characters weighs less than a longer one, so a short query
  finds a city by its start before a country by a run inside one of its names (D-269).

- Search importance is one scale for countries, regions and cities (D-266): a region or a
  country weighs its people as a city does, so "Var" finds Varanasi and "Cal" California
  among the first; a settlement of a million people ranks as a city; a place sharing its
  country's name ranks just below the country ("Luxembourg" gives the country first).
- A second-level unit ranks just below a city of its own name that it holds, as a first-level
  one did ("Varanasi" gives the city, then the district); India's municipal corporations
  rank as cities (D-264).

### Fixed

- Rebuilding one world part no longer renumbers another's places: each part holds its IDs
  under its own name in the shared registry, and a build that would retire more than 1% of
  its IDs stops unless `--allow-retire` (D-267). `data-2026.11.14` renumbered
  `baseline-lite` and `points-en-lite` once, to the released registry's IDs.

## [0.3.0] - 2026-11-13

Clients may install some parts of a pack (D-251 to D-254): some languages' names, one outline
part. New zone-rule queries (D-262). The pack format is unchanged: data releases read by
`0.2.0` read here too.

### Breaking

- Public structs gained fields, so a struct literal without `..Default::default()` no longer
  compiles: `LayerOptions::installed_parts`; in `teigeodata-sync`, `Entry::parts`,
  `Installed::parts`, `Record::parts` and `SectionSpan::part`.
- `teigeodata-sync` methods that took `Components` take `impl Into<Selection>`
  (`Install::new`, `Entry::size_of`, `Frames::plan_install`, `plan_components` and the
  install functions); a `Components` value still works, a closure or turbofish naming the
  old type does not.
- The store's journal is version 3 when a pack holds some parts only; `0.2.0` refuses such a
  store.

### Engine (`teigeodata-engine`)

- `LayerOptions::installed_parts` (`Parts`, re-exported): read only some parts of the split
  kinds, as a client that installed them (D-251). `Parts::ALL.languages(&[en])` reads the
  base names and the part that holds English (`*` when English has no part of its own);
  `.outline(part)` reads that outline part alone, whatever `outline_level` and
  `outline_tolerance_m` say.
- `Engine::tz_zones` (the zone names the rules hold) and `Engine::tz_rules_start` (the
  earliest instant a zone's rules cover, its first transition) (D-262).

### Format (`teigeodata-format`)

- `component::Parts` and `component::MAX_LANGUAGES`: which parts a client takes, with
  `holds(kind, part)` and `resolve` (D-251).
- `component::Selection`: components and parts together, `From<Components>` (D-252).
- `names::Origin::Standard` (code 10): a name from a standard's locale data, such as CLDR's
  territory names (D-257). Older readers read it as `Other(10)`.
- `tz_rules::Zone::first_transition` (D-262).

### Sync (`teigeodata-sync`)

- Installs of some parts (D-252): `Install::new`, `fetch::install`, `install_from`,
  `update_from` and `Store::install` take `impl Into<Selection>` (a `Components` still installs
  every part); `Installed::parts` and `Record::parts` hold the parts as resolved on the pack,
  `Record::selection()` both. `Frames::plan_install` and `plan_components` take a selection;
  `SectionSpan` has the section's `part`.
- The catalog lists each part of a split kind but the base (D-253): `Entry::parts`
  (`PartSize`: component, part, wire and disk bytes, and an outline's tolerance in metres),
  measured by `Measured::catalog_sizes`; an optional `parts` field of `custom`, so older
  readers ignore it. `Entry::size_of` takes `impl Into<Selection>` and counts only the parts
  held.
- `Entry::outline_for(level, tolerance_m)`: the outline part with the fewest bytes on the
  wire that holds the admin units of `level` and coarser within the tolerance (the base when
  none does, D-254).
- The store's journal is version 3 when a pack holds some parts only (each record's `parts`),
  so a reader that knows only 2 refuses it; it stays 2 otherwise.

### Command line (`tgd`)

- `tgd install --names LANGS --outlines LEVEL[@Mm]`: install only some languages' names and
  one outline part (D-254). Each replaces what is installed of its kind. `tgd catalog --json`
  lists each part (`parts`); `tgd status` shows the installed parts.

### Data

- Countries are named in Nepali from CLDR 49 (`cldr-49-ne`), with CLDR's short and variant
  forms as alternate names: 237 of 248 countries and disputed areas, up from 160 (D-257).
- India's districts (762) and statutory towns (4,128 of 5,050, each placed within its
  district) join the world part, from LGD, GeoNames and a Wikidata crosswalk (D-259).
- The world part keeps its IDs across builds: `forge world` reads the last build's registry
  and refuses to number afresh unless `--fresh-ids` is given (D-260); `forge make` too, and
  every data release carries its ID registries as `ids-<build>.tar.gz`, restored by
  `cargo xtask ids restore` (D-261).
- India's placed towns carry their official name in their state's language from LGD (1,781
  towns: Hindi 672, Tamil 474, Marathi 331 and seven more); typed-twice marks are collapsed
  and malformed vowel pairs refused (D-263).

## [0.2.0] - 2026-11-12

Packs may now split a section kind into parts (ADR-0040): names and search by language, outlines
by admin level. Packs built with this version need its readers.

### Engine (`teigeodata-engine`)

- `LayerOptions::outline_level`: read the outlines of one admin level and the coarser ones,
  from the pack's outline part (D-249); admin units below it are not found at a point.
- `LayerOptions::outline_tolerance_m`: read the outlines simplified to the coarsest level of
  detail the pack publishes within that many metres (20 m and 100 m, D-250); 0 for full
  resolution.
- `search` finds a place from 3 or more code points inside a word of any of its names, in any
  script ("mandu" finds Kathmandu, "माडौं" काठमाडौं), ranked below exact, prefix and one-edit
  matches (bonus 20). It needs the new `substring` component (with `search`) installed and the
  engine feature `substring` (on by default) (D-199).
- Postal codes (feature `postal`, D-200): `postal_lookup(code, scheme)`, `postal_prefix`,
  `postal_codes_of(place)`, `postal_at(point)` and `postal_replacement`, returning
  `PostalCode` (scheme, code, kind, validity, place).

### Format (`teigeodata-format`)

- Section `SUBSTRING` (kind 23), the word-suffix keys in SEARCH's layout, and component
  `substring` (13); `search::encode_substring` and `teigeodata_names::substring_keys` (D-199).
- Section parts (ADR-0040, D-246): a table entry's `part` key; `Pack::part`, `Pack::parts`,
  `Writer::pack_parts`, `Part`.
- Names and search by language (D-248): `names::LanguageParts`, `names::encode_part`,
  `NameSet`, `Search::open_part`, `SearchSet`, `search::substring_part_from_names`.
- Outlines by admin level (D-249) and level of detail (D-250): `Part::outline(deepest, lod)`
  is `deepest + 1 | lod << 8`, `None` past `Part::MAX_OUTLINE_LEVEL` (30) or lod 6;
  `Part::outline_levels`; `Kind::is_outline`; `SectionLabel`; `extract::outlines`;
  `ARCS_OVERVIEW` takes parts; `check` reads each outline part on its own arcs.

### Sync (`teigeodata-sync`)

- `Store::derive` builds every part of a derived component; its `build` takes
  `&[DeriveInput]` (kind, part, items, bytes) and returns `(Part, bytes)` per part.

### Breaking

- Country packs built from this version split NAMES, SEARCH and SUBSTRING by language. A reader
  from 0.1.0 refuses them (it sees a non-zero reserved byte); upgrade the crates with the data.
- `LayerOptions` has a new field, `outline_level`: build it with `..LayerOptions::default()`.
- `teigeodata_engine::derive::substring` takes `(Kind, Part, u64, Vec<u8>)` inputs and returns
  one `(Part, bytes)` per NAMES part; `Store::derive`'s `build` likewise.

### Command line (`tgd`)

- `tgd postal <code>` (or `--prefix`, or `<lat> <lon>`) prints codes and their places; `--json`.

## [0.1.0] - 2026-11-11

The first tagged version. Everything below is new.

### Engine (`teigeodata-engine`)

- Layered engine over any number of packs, each a layer with precedence (priority, release,
  specificity, address) and an embedded world baseline (feature `baseline`).
- Queries: `get`, `reverse`, `within_radius`, `nearby`, `within_bbox`, `parent`, `ancestors`,
  `children`, `name`, `search` (exact, prefix, fuzzy and phonetic, across scripts), `admin_at`,
  `countries_near`, `missing_packs`, `tz_at`, `tz_of`, `elevation`.
- `elevation_at(point)` returns a `Height`: metres above EGM2008, the DEM product, its sample
  spacing in metres, the datum and the layer (D-192, D-195).
- `bbox_of(place)` and `boundary(place, lod, &mut Outline)`: a place's extent and its outline at
  a level of detail (D-194).
- `iso_code(place)` and `by_iso(code)`: ISO 3166-1 and 3166-2 codes of countries and level-1
  units (`NP`, `NP-P3`), from per-country data files (D-197).
- Zone rules from the outlines' own tzdb release (feature `tz-rules`, D-196):
  `tz_offset_at(zone, instant)` (offset, abbreviation, DST, before the first rule),
  `tz_local_candidates(zone, civil)` (`Unique`, `Gap`, `Overlap`), `tz_version()` and
  `tz_offset_at_point(point, instant)`.
- One Cargo feature per data component, so an app carries only the readers it uses (D-137).
- Lazy verification of every section against the signed pack (D-078).

### Sync (`teigeodata-sync`)

- Signed catalog, verified downloads, resumable partial installs of chosen components, section
  re-download updates, and a store of installed packs.

### Command line (`tgd`)

- `tgd` answers every engine query, installs and updates packs, and prints JSON with `--json`.
- `tgd elevation <lat> <lon>` prints the height with its DEM product and spacing.

### Format (`teigeodata-format`)

- The `.tgd` pack format: a signed, canonical, deterministic column-table file with lazy
  per-section verification. Specified in
  [docs/04-architecture/pack-format.md](docs/04-architecture/pack-format.md).
- The DEM section states each grid's source product (column 15 `SOURCE`, D-195).
- New sections: `ISO_CODES` (kind 21, `admin`) and `TZ_RULES` (kind 22, component `tz-rules`),
  with the manifest tag `tzdb`. Older readers skip them.

### Breaking

None: no version was released before this one.

[Unreleased]: https://github.com/teispace/teigeodata/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/teispace/teigeodata/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/teispace/teigeodata/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/teispace/teigeodata/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/teispace/teigeodata/releases/tag/v0.1.0
