---
title: "REGION rivals: tzf-rs full data and other exact point-in-timezone lookups"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-02-07
---

# REGION rivals: tzf-rs full data and other exact point-in-timezone lookups

## 1. Question

B9 builds REGION, an exact point→zone index on timezone-boundary-builder (tbb) 2026f
`timezones-with-oceans` on the 1e-7° grid ([ADR-0029](../../03-decisions/adr/ADR-0029-coordinate-grid.md)).
The [phase plan](../../07-planning/phases/phase-2.md) names "`tzf-rs` 2.3.0 full data" as the
speed rival; [spike S2](../benches/s2-region-index.md) measured only its default simplified data.
Which rival configurations should B9 benchmark, how are they built, and how do they treat
borders and overlaps?

## 2. Summary and recommendation

- `tzf-rs` "full" data is **not** the tbb geometry at our resolution: every `.tzb` file stores
  coordinates as `i32` degrees × 100,000 (1e-5°, about 1.1 m), and the ray cast is in `f64`.
  It is unsimplified (the same 444 zones and 8,296,476 points as our input) but rounded by up to
  0.5e-5° per axis. Differential tests against it must expect mismatches within about 1 m of a
  border; it is a **speed** rival, not an exactness reference.
- The `full` Cargo feature cannot build from crates.io, and both documented recipes need a git
  source, which our `deny.toml` forbids (`unknown-git = "deny"`). **Build the rival from bytes
  instead:** `tzf-rs` from crates.io with `default-features = false`, plus the pinned `full.tzb`
  release asset loaded with `DefaultFinder::from_tzb` / `EmbeddedFinder::from_tzb` (§5).
- Benchmark four rival rows: `DefaultFinder` full (`get_tz_name`, and `get_tz_names` as the
  polygon-exact path), `EmbeddedFinder` full (queries the bytes in place, closest to our
  zero-copy design), and `DefaultFinder::new()` lite as the S2 baseline.
- Second exactness reference: `geo` 0.33.1 `IntervalTreeMultiPolygon` per zone on `f64`
  coordinates holding integer 1e-7 units, where its robust `orient2d` is exact.
- `timezonefinder` 9.2.0 (Python, unsimplified, 1e-7): a one-off agreement check and a Q-013
  input, not a timing rival.

## 3. Findings

**tzf-rs 2.3.0 (2026-11-02).** Features (crate manifest): `bundled` (default, lite data from
crates.io `tzf-dist`), `full = ["dep:tzf-dist-git", "tzf-dist-git/full"]`, `clap`,
`export-geojson`. The [README](https://github.com/ringsaturn/tzf-rs/blob/v2.3.0/README.md)
says the `full` feature gives a "Full-precision `.tzb` (~14 MB) plus
`DefaultFinder::new_full()`. The data is git-only" and is "Mutually exclusive with `bundled`".
`Cargo.toml.orig` adds: "`full` is not a crates.io-resolvable feature and never has been". The
README's recipe B still names tag `v0.0.2026-c-tzb1`; the data matching 2.3.0 is `v0.0.2026-f`.

- **Finders.** `DefaultFinder` expands the geometry into `geometry-rs` 0.5.1 `I32Polygon`s at
  load. `get_tz_name` first reads the FUZZY preindex (map tiles up to zoom 13 that lie in one
  zone), then a dense 1°×1° GRID of candidate zones, then a YStripes-accelerated ray cast
  (`enable_y_stripes: true`, R-tree off, rings under 32 segments scanned linearly;
  `src/finder.rs`). A GRID cell with one candidate returns it without a polygon test.
  `get_tz_names` skips the preindex and tests every candidate. `EmbeddedFinder` decodes the
  varint geometry from the bytes per query, with an open-time index of chunk skip blocks and
  latitude stripes.
- **Full data** ([tzf-dist `STATS.md`, 2026f](https://github.com/ringsaturn/tzf-dist/blob/v0.0.2026-f/STATS.md)):
  input "timezones=444 polygons=1356 holes=792 points=8296476", deduplicated and compressed, no
  simplification. `full.tzb` is 15,409,489 bytes; header read on 2026-11-07: magic `TZFB`,
  format 1.1, `coord_scale` 100000, `data_version` `2026f`, 444 zones.
- **Quantisation.** [Format reference](https://project-tzf.ringsaturn.me/docs/reference/embedded-binary-format/)
  §1: "Coordinates: signed int32, degrees multiplied by 100000"; §7: "Scale once without
  rounding: x = float64(lng) * 100000.0". `build_item` uses `I32RaycastMode::Float`.
- **Memory and open time** (tzf-rs CHANGELOG v2.0.0, Apple M3 Max, data 2026c, 2026-08-28):
  `DefaultFinder` full opens in 66 ms cold with peak RSS 212.8 MiB (lite 44.1 MiB).
  `EmbeddedFinder` full answers edge-city queries in 2.74 µs mean (v2.1.0, 64-point chunks
  1.65 µs); its footprint is the file plus a few hundred KB.
- **Borders.** `Item::contains_point` uses `contains_point_allow_on_edge`: "a query that lands
  exactly on a shared border must belong to both neighbours" (tzf-rs issue 207). The format
  reference: an exterior ring contains a point on its segment; a hole ring does not.
- **Overlaps.** `get_tz_name` returns "the first containing candidate" in stored order;
  `get_tz_names` returns all, sorted by name bytes. What the stored order is (input order or
  name) is **unverified**; no area or perspective rule.

**timezonefinder 9.2.0 (2026-10-22), data `timezonefinder-data` 4.2026.6 (2026-11-01).**
"The boundary polygons are never simplified"; queries scale to `i32` × 10^7; a grid of 0.75°
cells (0.1875° where zones meet) gives candidates; integer ray cast in C (`int64_t`).
`configs.py`: "timezone-boundary-builder publishes at most six decimal places", so the 1e-7
grid is lossless. Borders ([`test_point_in_polygon_boundary_convention.py`](https://github.com/jannikmi/timezonefinder/blob/master/tests/test_point_in_polygon_boundary_convention.py)):
"a polygon owns a point on its boundary exactly when its interior lies immediately west of the
point - or, where the boundary runs due east-west, immediately south", so neighbours never both
own a border point; −180° is read as +180°. Overlaps ([data format](https://timezonefinder.readthedocs.io/en/latest/data_format.html)):
`timezone_at()` returns "the zone with the smaller spherical area", `timezones_at()` all.

**geo 0.33.1 (2026-04-20).** `IntervalTreeMultiPolygon` (since 0.32.0) indexes all ring segments
by y-interval and runs a winding number with the robust `orient2d`. Its on-boundary result is
crate-private: the public `Contains<Coord>` is interior-only, and the unindexed
`MultiPolygon::coordinate_position` returns `Inside`, `OnBoundary` or `Outside`. `orient2d` is
exact on `f64`, and 1e-7 units (at most 1.8e9) are exact `f64` integers; `i64` would take the
simple kernel, whose products can overflow.

**tbb 2026f** (2026-11-01) is still the latest release on 2026-11-07.

## 4. Options compared

| Option | Method | Exact on our grid | Licence | Version | Verdict |
|---|---|---|---|---|---|
| `tzf-rs` `DefaultFinder` full | Preindex tiles, 1° grid, YStripes `f64` ray cast on 1e-5 ints | No (1e-5 rounding) | MIT, data ODbL | 2.3.0 | Speed rival |
| `tzf-rs` `EmbeddedFinder` full | Same file queried in place | No | MIT, data ODbL | 2.3.0 | Footprint rival |
| `tzf-rs` lite | Douglas–Peucker 0.001° | No (≤ 111.7 m) | MIT, data ODbL | 2.3.0 | S2 baseline |
| `geo` `IntervalTreeMultiPolygon` | y-interval tree, winding, robust `orient2d` | Yes, with integer-valued `f64` | MIT/Apache-2.0 | 0.33.1 | Second reference |
| `timezonefinder` | 0.75° grid, `int64` ray cast, 1e-7 | Yes, half-open west/south | MIT, data ODbL | 9.2.0 | Agreement check, Q-013 input |
| `rtz` | geo PIP behind a 1° cell cache; `unsimplified` feature, default ε 0.0001; committed data from 2024-08-08 | No by default | MIT | 0.10.0 (2026-07-23) | Skip: stale data |
| Others | `spatialtime` 0.3.0 (2025-12-23, FlatGeobuf R-tree), S2 `S2ShapeIndex` v0.14.0 (2026-04-23, C++, spherical edges), `tg` v0.8.2 (2026-09-09, C, per-polygon YStripes), `s2` 0.2.0 (Rust, no shape index) | — | — | — | Skip |

## 5. What we build ourselves, and why

The harness lives in the B9 benchmark crate (dev-dependency only, never shipped):

```toml
[dev-dependencies]
tzf-rs = { version = "=2.3.0", default-features = false }
geo = "0.33.1"
```

`full.tzb` comes from the `v0.0.2026-f` release asset, pinned by SHA-256 and fetched like any
other benchmark input (not committed; ODbL):

```text
https://github.com/ringsaturn/tzf-dist/releases/download/v0.0.2026-f/full.tzb
size 15409489, MD5 cee514b502c2855a0288c48cf2c1b5c1 (upstream checksums.md5)
SHA-256 75513a1608b975cae64daf6a12a1ced9a05b3a31bc5fd809270ecdd7f535d8a3
```

```rust
use tzf_rs::{DefaultFinder, EmbeddedFinder};

let bytes = std::fs::read(full_tzb_path)?;                // verified SHA-256 first
let full = DefaultFinder::from_tzb(&bytes)?;              // expands; ~213 MiB RSS
let full_in_place = EmbeddedFinder::from_tzb(bytes)?;     // Vec<u8>, queried in place
assert_eq!(full.data_version(), "2026f");
let one: Option<usize> = full.get_tz_index(lng, lat);     // preindex fast path; lng first
let all: Vec<&str> = full.get_tz_names(lng, lat);         // polygon-exact, allocates
```

Lite needs `features = ["bundled"]` and `DefaultFinder::new()`; it is a separate run, because
`full` and `bundled` cannot share a build and `bundled` embeds its own data.

The `geo` reference: one `IntervalTreeMultiPolygon` per zone from coordinates in 1e-7 units as
`f64`, queried with `contains(&coord)`; a point no zone contains goes to the unindexed
`MultiPolygon::coordinate_position` for `OnBoundary`. Answer sets must equal REGION's off edges.

Report for each rival: ns per query (uniform world, near-edge, real places), open time, peak
RSS, on-disk bytes, and agreement with REGION, all under [the benchmark method](../../06-engineering/benchmarks.md).
Use `get_tz_index` for timing so no string is touched.

## 6. Impact on the plan

- Phase 2 §2.2: the rival is "`tzf-rs` 2.3.0 with `full.tzb` 2026f loaded from bytes"
  (`DefaultFinder` and `EmbeddedFinder`), and `geo` 0.33.1 is the second exactness reference.
- Q-013 gains two data points: tzf-rs takes stored order, timezonefinder the smaller area.
- On-edge rule options seen: both neighbours own the point (tzf-rs), or exactly one by a
  west/south half-open rule (timezonefinder). REGION returns a set, so the first is natural.

## 7. Risks and open items

- Agreement with tzf-rs full will not be 100 %: 1e-5 rounding and `f64` ray casting move
  borders by up to about 1 m. Report its disagreements by distance to the nearest edge.
- tzf-rs FUZZY tiles: how they answer in overlap areas (Shanghai/Urumqi) is **unverified**.
- `from_tzb` relies on format 1.1 staying stable; a later tzf-rs may reject old files.
- tzf-rs timings in its own docs are on data 2026c; re-measure on 2026f.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| `tzf-rs` | 2.3.0 (2026-11-02) | 2026-11-07 | crates.io, source |
| `tzf-dist` | 0.0.2026-f (2026-11-01) | 2026-11-07 | crates.io, GitHub release |
| `geo` | 0.33.1 (2026-04-20) | 2026-11-07 | crates.io, source |
| `timezonefinder` | 9.2.0 (2026-10-22) | 2026-11-07 | PyPI |
| `timezonefinder-data` | 4.2026.6 (2026-11-01) | 2026-11-07 | PyPI |
| timezone-boundary-builder | 2026f (2026-11-01) | 2026-11-07 | GitHub releases |
| `rtz` | 0.10.0 (2026-07-23) | 2026-11-07 | crates.io |

## 9. Sources

- [tzf-rs README and source, v2.3.0](https://github.com/ringsaturn/tzf-rs/tree/v2.3.0), accessed 2026-11-07.
- [tzf-dist v0.0.2026-f](https://github.com/ringsaturn/tzf-dist/tree/v0.0.2026-f) (README, `STATS.md`, `Cargo.toml`), accessed 2026-11-07.
- [TZF embedded binary format](https://project-tzf.ringsaturn.me/docs/reference/embedded-binary-format/), accessed 2026-11-07.
- [timezonefinder](https://github.com/jannikmi/timezonefinder) README, `configs.py`, boundary test; [docs](https://timezonefinder.readthedocs.io/en/latest/), accessed 2026-11-07.
- [geo 0.33.1 source](https://docs.rs/geo/0.33.1/geo/), `indexed/interval_tree_multipolygon.rs`, `CHANGES.md`, accessed 2026-11-07.
- [timezone-boundary-builder releases](https://github.com/evansiroky/timezone-boundary-builder/releases), accessed 2026-11-07.
- [rtz](https://github.com/twitchax/rtz), [spatialtime](https://github.com/moranbw/spatialtime), [tg](https://github.com/tidwall/tg), [s2geometry](https://github.com/google/s2geometry/releases) READMEs and releases, accessed 2026-11-07.
