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

### Breaking

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
