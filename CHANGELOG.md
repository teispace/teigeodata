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

[Unreleased]: https://github.com/teispace/teigeodata/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/teispace/teigeodata/releases/tag/v0.1.0
