# Changelog

Every change to the crates, by version. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/). From 1.0.0, [what 1.0 promises](docs/06-engineering/semver.md)
says what each surface keeps across minor releases (the Rust API, the C ABI, the bindings, the
pack format, the catalog, the HTTP and MCP servers, and `tgd`). Before it, the 0.x rules
applied: a minor release could break the public API, and every break is listed under
**Breaking**. How releases are cut: [docs/06-engineering/releases.md](docs/06-engineering/releases.md).

## [Unreleased]

### Added

- CLI: `tgd delta <older> <newer> -o <out>` writes a delta pack (D-391): the places that
  changed between two releases of a pack, read as a layer above the older. On Nepal's app pack
  it is about 1 % of the asset. Pairs that differ in what a delta cannot carry are refused;
  changed query rules travel in the delta (D-394).
- Engine: delta layers. `Layer::delta_of` names the release a delta is read above; admin units
  at a point, countries near a point and the timezone at a point answer from a delta and its
  base together; a base answers search by its delta's query rules.
- Format: manifest tag `delta_of` (0x000A) and `extract::rows_outlined`, a slice whose
  ancestors are stubs without outlines.
- Sync: delta packs ship (D-395). The catalog lists a pack's deltas in `custom.deltas`
  (`catalog::Delta`); an update takes the delta from the installed release when it costs at most
  a quarter of the pack, else updates by section; `Store::install_delta` installs one, crash-safe,
  in the journal's `deltas` (version 4, written only with a delta); `store::current`,
  `Client::deltas`, `Client::delta_path` and `Engine::follow_with` open and report it. `tgd status`
  shows `delta_of`. `cargo xtask tuf release --delta` lists a delta in its pack's entry.
- CLI: `tgd inspect --json` gives each section's `part` (`base`, `*`, a language); the table
  names it (`NAMES \`ne\``, `ARCS to level 2 at lod 1`).

### Fixed

- Sync: a pack the catalog does not list is refused with the packs it lists at that address,
  so `tgd install app-np` (read as `app-np.places.tgd`) points to `app-np.baseline.tgd`.
- CLI: `tgd diff` compares a kind split into parts part by part. It compared only the base
  part, so a changed language part of NAMES, SEARCH or SUBSTRING was counted as unchanged.

### Changed

- Forge: each build ships its own transliteration model (D-399). Main's also trains on
  OpenStreetMap's Latin and Nepali names: on official names 55.7 % folded exact and CER 0.088,
  against 51.9 % and 0.099 for lite's, which is unchanged. `forge xlit-eval --country <cc>
  [--lite]` gates both, and the data build runs it.

## [1.2.0] - 2026-11-23

### Added

- Forge: census populations (D-386). A country's `census.yaml` joins a census table, read
  straight from its xlsx, to an official list's units row by row, with every row checked by
  name. Nepal's main build gives the 753 local levels their 2021 census counts; their wards
  are scaled to them and the levels above re-summed.
- Sync: a catalog entry's `file` may start with one folder,
  `data-2026.11.22.2/np.places….tgd.zst`, fetched at the mirror base joined with it, so each
  release's assets can live in their own immutable GitHub release under one base;
  `Entry::name` is the asset's name without it, which the installed pack is named after.
  Readers before 1.2 refuse such an entry (D-384).
- xtask: the catalog keys in an encrypted vault and a one-command ceremony (`tuf ceremony`,
  `*.key.age` keys, `tuf secret`), and `tuf release --folder` (D-383, D-384).
- Engine and every binding: a place's class (`Engine::class`; a nullable `class` column in
  place blobs; `class` in the server's JSON, GeoJSON, text and MCP output). Landmarks are
  `locality` places with a class such as `palace`, `temple` or `airport`, one growable list for
  every country (D-381). The server's Rust `output::Record` keeps its 1.0 fields, because a
  caller may build it with a literal; Rust callers read a class with `Engine::class` (D-388).
- Engine: `Layer::from_sections` opens a pack from its head and some of its sections, each in
  its own buffer, so the sections not read stay out of memory; format:
  `Pack::open_sections` and `Pack::check_blocks` (D-370).
- Sync: the catalog refresh (`meta`) and the store policy (`policy`) as one async core over
  an I/O trait, which the native client runs with `meta::block_on`; the client's API is
  unchanged (D-369); the pack driver too, `fetch::AsyncTransport`, `install_from_async` and
  `update_from_async` (D-371); the store too, `store::AsyncStore` over `store::AsyncFs`, with
  `Store` its synchronous facade, and `store::parse_journal` for readers (D-372).
- Wasm: a reader of an installed pack, `installedSections` and `Engine.openInstalled`, which
  open only the sections a journal record's components hold; `Engine.openSections`; Rust API:
  the same in `Engine::open_installed`, `Engine::open_sections`, and format:
  `PackHead::holds` (D-373).
- Sync: the store's client as one async core, `session::Session` with `session::init`, which
  the native `Client` runs; `fetch::Ready` and `store::SyncFs` are public (D-374).
- Wasm: the browser store, `import { Store } from "teigeodata-wasm/store"`: the native store's
  client over a host's files (OPFS sync access handles) and `fetch`, with the native store's
  results; `parseJournal`. API: `rows` (feature `store-core`), the store's results as blobs
  for every store; sync: `store::render_journal`, `policy::path_of` (D-375).
- Wasm: `store-worker.js`, the browser store's worker over the origin's private file system,
  and `opfs.js` in every package: `opfsHost`, `openInstalled` (the installed packs in any
  engine, each file held by a shared Web Lock) and `storeWorker` (D-376).

### Changed

- xtask: `mirror` uploads to the release each source's `mirror_url` names, creating a new
  one as a draft and publishing it once filled. Releases here are immutable, so new inputs go
  to `sources-<date>` (D-387). `check-data` allows an http upstream for a mirrored source.
- Names: the search fold turns Devanagari's न with a virama before a stop of any class into the
  anusvara, as Nepali and Hindi write a loanword's n (`लन्डन` finds लण्डन's London,
  `काउन्टी` = `काउंटी`). Packs keep each such name's key under the 1.0 fold too, and the engine
  searches a query under it too, so packs and engines of 1.0 and 1.1 still meet; new:
  `fold_1_0` (D-378).

### Fixed

- Sync: a store opens while a pack file it no longer lists cannot be removed yet (a reader
  holds it), and a commit cut between a removal and a move is finished on the next open
  instead of leaving the store empty (D-376).

## [1.1.0] - 2026-11-17

Reviewed names as their own tier of the names chain, Government of Nepal data admitted and
credited, and the fixes found publishing 1.0.1. The first release published from CI by
trusted publishing on every registry.

### Fixed

- Dart: the build hook's helpers moved to `lib/src/hook/`, as pub.dev accepts only
  `hook/build.dart` and `hook/link.dart`; the pub.dev 1.0.1 is published from that commit (D-361).
- Release: npm publishes the tarballs by path (`./dist/…`), platform packages first.

### Added

- Name origin `reviewed` (11): a name a person corrected against a cited source.
- The forge's reviewed-names tier: `data/countries/<cc>/reviewed-names.tsv`, written by
  `forge reviewed-import`, applied by stable source key with a guard, gate G-reviewed on
  stale rows; only attribution-only sources reach the lite build (D-359).
- Licence class `government`: a government's own work with no open licence stated, admitted
  per source by a recorded decision and credited; Nepal's Election Commission lookups and
  Constitution are the first (D-360). Reviewed names credit their sources in the pack's
  notices.

## [1.0.1] - 2026-11-16

The first release on every registry (crates.io, npm, PyPI, pub.dev, Maven Central); Swift and
Go install from its tag as before.

### Changed

- The npm packages are `teigeodata`, `teigeodata-wasm` and `teigeodata-wasm-lookup` (the
  names on crates.io, PyPI and pub.dev), owned by the Teispace organisation; the per-platform
  addons the main package installs stay scoped, `@teispace/teigeodata-<platform>`. Neither was
  on npm before this release.

### Fixed

- `tgd extract --components` of a pack with several parts of one kind or with block hashes
  (any section of 1 MiB or more) failed with "a section kind was given twice": it now keeps
  every part and writes its own block hashes.
- `tgd extract --bbox` keeps only the terrain tiles that touch the box, copied as stored,
  instead of the whole grid: the Kathmandu Valley out of Nepal's lite pack is 1.5 MB, not
  70 MB, with the same heights inside the box.

## [1.0.0] - 2026-11-16

The first stable release. Every public surface now has its promise and its gate
([what 1.0 promises](docs/06-engineering/semver.md)); the pack format is 1.0 and older packs
stay readable; every public enum that may grow is `#[non_exhaustive]`; the 1.0 API review
renamed the last parameters that broke the naming rule (see **Breaking**).

### Added

- Each place's population (D-351): packs carry it in COMPONENTS (component 1, a count and
  whether a source stated it or a population grid modelled it); `Engine::population` (feature
  `extra`, on by default) returns `Population { people, source }`; every place result has the
  nullable columns `population` and `population_kind` in every language, and `tgd`, the HTTP
  server and MCP print `population` and `population_kind`. `extract::merge` keeps every
  component of COMPONENTS. Both WebAssembly packages build `extra` (feature of
  `teigeodata-wasm`); every binding's parity suite checks both columns.
- `teigeodata-engine` feature `fuzz`: `fuzz::engine`, every query on a pack with edited
  sections whose hashes hold, run nightly by libFuzzer (`fuzz/fuzz_targets/engine.rs`) and on
  seeded inputs by `cargo test` (D-331); `teigeodata_format::fuzz::edit_section` builds such
  packs, and the edited sections' full check runs too.
- `teigeodata_sync::fuzz::trust` (feature `fuzz`, now with `repo`): the TUF update chain and
  the catalog on a repository with one role attacked, nightly as the `trust` target (D-331).
- `teigeodata_ffi::fuzz::abi` (feature `fuzz`): every engine function of the C ABI with fuzzed
  arguments, nightly as the `ffi` target; `teigeodata_server::fuzz::mcp` (features `fuzz` and
  `mcp`): every MCP tool with arguments from its own input schema, and resource reads, nightly
  as the `mcp` target (D-331).
- The weekly fuzzing campaign (`fuzz-campaign.yml`): every target for 3 hours with coverage
  reports, and its gate `cargo xtask fuzz-coverage`, which fails unless every section kind's
  decoder is reached (D-331).
- `teigeodata_server::http::Config` gains `header_timeout`, `max_connections` and
  `max_in_flight`, and `tgd serve` gains `--max-connections` and `--max-in-flight` (D-334).
- `cargo xtask unsafe-audit`: the inventory of every `unsafe` in shipped code with its argument
  and the tests that reach it (`docs/06-engineering/unsafe-inventory.md`), and the
  [threat model](docs/06-engineering/threat-model.md) (D-333).
- `teigeodata_format::check::sections_where`: the full check of the sections a caller selects.
- CodeQL code scanning (`codeql.yml`) and the OpenSSF Scorecard (`scorecard.yml`), weekly.
- Extension columns (D-335): a column-table column with an ID from `0x8000`
  (`teigeodata_core::table::EXTENSION`) is skipped by a reader that does not know it, so a
  format minor can add optional columns; `Table::extensions` lists them. COMPONENTS IDs are 1
  to 127. `teigeodata_format::fuzz::{bodies, write_again}` rebuild a pack from its sections.
- A result `Reader` of one shape in every language (D-322): new in Node.js, WebAssembly,
  Swift, Dart, Java (Kotlin), C (`teigeodata_reader.h`) and C++ (`teigeodata_reader.hpp`),
  both header-only and in the C package (`rows`, `columns`, `has`, `numbers`, `strings`, `nulls`, `column`,
  `records`, iteration); Go's gains `Has`, `Column` and `Records`, Python's `has`, `column`,
  `records` and iteration, .NET's `BlobReader` `Has`, `Numbers`, `Column`, `Records` and
  iteration.
- `cargo xtask docs-examples`: runs every language's example program on the pinned public pack
  and checks its output (D-322).
- The API, HTTP and MCP reference of the site, generated: the API's from each binding's own
  declaration macros by `cargo xtask bindings`, the servers' from `teigeodata_server::http::ENDPOINTS`
  (a route table a test holds to the parser) and `mcp::tool_definitions` and
  `mcp::resource_templates` (D-323).
- `Engine::boundary(id, lod)` (an outline as a GeoJSON Feature, with its level, tolerance and
  vertex count), `Engine::distance(from, to)` between two places, and `bearing_deg` in the
  language API and every binding, with parity values checked in each (D-324).
  `teigeodata_engine::Outline::write_geojson` and `write_point_geojson`: the GeoJSON writer the
  servers used, now one writer for the servers and the bindings.
- `cargo xtask data-pages`: the site's coverage, country and measured pages, generated from the
  pinned pack, `data/countries/` and the gates' records, with `--check` (D-325, D-326).
- The site's playground (`/playground`): the WebAssembly engine and the pinned pack in the
  browser, MapLibre drawing the pack's own outlines, every answer with its code and timing;
  `cargo xtask playground [--fetch]` writes its files, and `site/scripts/check-playground.mjs`
  runs it in Chromium in the docs workflow (D-328).
- `--record` on `package-wasm`, `sizes` and `load`, and `cargo xtask phone <layer.tgd>
  [--record]` (the phone-class bench with the example's own features): each writes its table,
  stamped with the day, commit, machine and load average, to `site/data/measured/` (D-326).
- `cargo xtask profile <pack>… [--suite] [--secs]`: each query suite under Instruments' Time
  Profiler, one suite per process, with the largest frames by self time and under our own
  frames; `cargo xtask scaling <pack> [--record]`: one shared engine at 1, 2, 4 and 8 threads,
  failing under 1.5× at four; the engine's `profile` and `scaling` examples (D-338).
- `cargo xtask accuracy <cc> <pack>`: a shipped pack scored against the blind labels of
  `tests/golden/<cc>-places.csv` and its second pass, written as the generated
  `data/countries/<cc>/ACCURACY.md` (D-339).
- Suite C, the competitive report: `tools/competitive` and `competitive.yml` ask teigeodata,
  `tgd serve`, Nominatim, Photon, GeoNames' own points and `tzf-rs` the same questions (D-340).

### Changed

- `teigeodata_format::Sorted::find` interpolates in `u64` and uses `u128` only when the
  product overflows: `get` −6% on Nepal lite, −4% on the world baseline. `teigeodata_core::bitpack::Packed::parse`
  checks a column's block directory as independent steps that vectorise: opening a pack −14%
  and −17% (D-338).
- `distance_m` calls the engine's `meters_between` instead of a second haversine of its own
  (D-324).
- `cargo xtask load` names each pack in bold and right-aligns its figures, so its tables read
  as Markdown (D-326).

- `cargo xtask load`: a load test of `tgd serve` and its `/mcp` on real packs with `oha`,
  gated by `tools/load/budget.toml` (requests per second, p99, footprint and its growth)
  (D-320); `cargo xtask sizes --tgd` reports the binary with and without its servers.

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
- `tgd mcp` (feature `mcp`, on by default) and `teigeodata_server::mcp`: the MCP server on
  standard input and output, for revisions 2026-07-28 and 2025-11-25; fourteen read-only tools
  (`search`, `reverse`, `nearby`, `get_place`, `lookup`, `admin_at`, `hierarchy`, `children`,
  `timezone`, `elevation`, `postal`, `boundary`, `distance`, `packs`) with input and output schemas, structured
  answers with their attribution, signed cursors for pages, and packs and places as resources
  (D-315).
- `/v1/hierarchy?id=` (a place's ancestors) and `/v1/children?id=&limit=` (D-315).
- `/v1/boundary?id=&lod=`: a place's outline as GeoJSON, at the finest level of detail within
  20,000 vertices unless one is asked; `/v1/distance?from=&to=`: metres and bearing between
  places or points. In MCP, the `boundary` tool links the outline as the resource
  `teigeodata://boundary/{id}?lod=` (D-317).
- `/v1/coverage?lat=&lon=&radius_m=` and the MCP tool `coverage`, on a server over the store:
  the countries at a point with the catalog's packs for it and their state, as `tgd locate`
  answers, read from the stored catalog without opening the store (D-318).
  `teigeodata_sync::client::catalog_at` reads it so; `PackState::of`.
- MCP `install` and `install_status`, on a server over the store: catalog packs installed into
  the store and served, as a task (`tasks/get`, `tasks/cancel`) for a client with the tasks
  extension or a handle polled with `install_status`, from one job registry. The size is
  confirmed by the person first (MRTR elicitation in 2026-07-28, `elicitation/create` for a
  2025-11-25 client on stdio), else the download must fit `max_bytes` (100 MB by default). On
  for `tgd mcp` (`--no-install` leaves it out), opt-in for `tgd serve` (`--allow-install`)
  (D-318).
- The official MCP conformance suite runs in CI against `tgd serve` for 2025-11-25 and
  2026-07-28 clients, every check gated against a reasoned baseline (D-319). It found, now
  fixed: `/mcp` on loopback accepts the server's own origin; `prompts/list` and
  `completion/complete` answer method not found (neither is declared); a missing resource's
  error carries `data.uri` (SEP-2164).
- `tgd serve` answers MCP at `/mcp` (stateless Streamable HTTP), refusing a foreign `Host`
  on loopback and any browser origin not configured with `--cors-origin` (D-316).
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

- Every result-blob reader refuses a blob with no column (D-349). Such a header could name billions of rows that no column bounds, and Go's `Reader.Records` would build a record for each until memory ran out. Found by the Go reader's fuzzer; no valid blob is affected.
- The documentation, read against the code for 1.0 (D-343): a reverse answer's `bearing` is from
  the place to the point, several `--pack`s follow the layer rule rather than their order, the
  Nominatim routes' supported parameters and differences are listed, and each language's guide
  names its own API (Rust's the typed engine, WebAssembly's `openBytes` only).
- `tgd verify --deep` and `teigeodata_format::check::sections` now run the `TZ_RULES` section's
  full check; it was skipped (D-335).
- `tgd serve` (`http::serve`) never timed out a slow request head or an idle connection, since
  `axum::serve` sets no hyper timer, and did not cap connections or requests in flight. It now
  bounds all four, and sheds requests past the cap with 503 and `Retry-After` (D-334).
- `bindings/fixtures/outline.tgd` stored its hole counter-clockwise, against the format's rule
  (`Polygons::check` refused it; queries re-orient, so no answer changes): the hole's arcs are
  now reversed. Found when the fuzz seeds were held to the full check.
- A pack whose DEM, ARCS or POLYGONS states an item count near 2^64, or whose DEM has a grid
  with an invalid tile side, step, datum, source, model or threshold, no longer panics a
  query: `Dem::open` checks every grid, model and threshold (O(grids)) and the three readers
  refuse a count with no room for its end boundary. Found by the new `engine` fuzz target
  (D-331).
- `tz_at` and every answer's `tz` column named no zone in a world timezone area (`tz` was null
  at New York or Shanghai, set only where a country's pack recorded its own area): a timezone
  area is now its own zone, whatever `PLACE_TZ` holds for its row (D-327).
- The MCP `search` tool said `strict` keeps every word in order; it ranks by how well the name
  matched first, as everywhere else. The place fields of every tool's output schema now say
  what each holds.
- `/packs` and MCP's `packs` gave a pack's length in bytes as its `places`; they give its
  number of places.
- The HTTP server's failures and `/packs` end with a newline, as every other answer does.

- Java and Kotlin: `new Engine()` (or `Store`, `Job`) as a program's first call threw
  `UnsatisfiedLinkError`: a nested class is initialized without its outer class, so the native
  library was loaded only by a call on `TeiGeodata` first. Every handle class now loads it
  (D-322).
- `@teispace/teigeodata-wasm` on Node.js loaded its browser entry: the package `exports`
  conditions were written sorted, `default` before `node` (D-322).
- `Shared::follow` (and `Engine::follow`) opens a store pack with the parts it was installed
  with (D-251); before, a pack of some parts was opened as whole, its parts not installed read
  as holes (D-311).

### Breaking

- The 1.0 API review (D-341): `Engine::within_radius` takes `radius_m`, not `meters`, as every
  other distance parameter ends in `_m` (and `tgd serve` calls it `radius_m`); in Swift the
  label is `radiusM:`, in Python the keyword `radius_m`, in .NET the named argument `radiusM`. `Engine::postal_prefix` takes
  `prefix`, not `input`. Positional calls are unchanged.
- 27 public enums are `#[non_exhaustive]` (D-337): errors such as `ErrorCode`, `ClientError`
  and `StoreError`; modes such as `Verify`, `Load` and the server's `Ask`; vocabularies such
  as `PlaceKind`, `Component`, `HeightSource` and `CodeKind`. A `match` on one outside its
  crate needs a `_` arm. New: `CodeKind::name` and `Health::name`.
- The pack format is 1.0 (D-336): packs written now have `format_major` and
  `min_reader_major` 1, so a reader before this release refuses them. This release reads
  every pack of earlier releases. `FORMAT_MAJOR`, `READER_MAJOR` and `FORMAT_MINOR` are 1, 1
  and 0. A client no longer downloads a pack whose catalog entry needs a newer reader.
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

[Unreleased]: https://github.com/teispace/teigeodata/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/teispace/teigeodata/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/teispace/teigeodata/compare/v1.0.1...v1.1.0
[1.0.1]: https://github.com/teispace/teigeodata/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/teispace/teigeodata/compare/v0.3.1...v1.0.0
[0.3.1]: https://github.com/teispace/teigeodata/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/teispace/teigeodata/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/teispace/teigeodata/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/teispace/teigeodata/releases/tag/v0.1.0
