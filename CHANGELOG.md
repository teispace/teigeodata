# Changelog

Every change to the crates, by version. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/) with its 0.x rules: before 1.0.0 a minor release may break the
public API, and every break is listed under **Breaking**. How releases are cut:
[docs/06-engineering/releases.md](docs/06-engineering/releases.md).

The stable surface is the public Rust API of `teigeodata-engine` and `teigeodata-sync`, the
`tgd` command line and its JSON output.

## [Unreleased]

### Engine (`teigeodata-engine`)

- `LayerOptions::installed_parts` (`Parts`, re-exported): read only some parts of the split
  kinds, as a client that installed them (D-251). `Parts::ALL.languages(&[en])` reads the
  base names and the part that holds English (`*` when English has no part of its own);
  `.outline(part)` reads that outline part alone, whatever `outline_level` and
  `outline_tolerance_m` say.

### Format (`teigeodata-format`)

- `component::Parts` and `component::MAX_LANGUAGES`: which parts a client takes, with
  `holds(kind, part)` and `resolve` (D-251).
- `component::Selection`: components and parts together, `From<Components>` (D-252).

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

[Unreleased]: https://github.com/teispace/teigeodata/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/teispace/teigeodata/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/teispace/teigeodata/releases/tag/v0.1.0
