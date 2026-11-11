# Changelog

Every change to the crates, by version. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[SemVer](https://semver.org/) with its 0.x rules: before 1.0.0 a minor release may break the
public API, and every break is listed under **Breaking**. How releases are cut:
[docs/06-engineering/releases.md](docs/06-engineering/releases.md).

The stable surface is the public Rust API of `teigeodata-engine` and `teigeodata-sync`, the
`tgd` command line and its JSON output.

## [Unreleased]

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

### Breaking

None: no version was released before this one.
