---
title: "Exact timezone lookup, and accurate place coordinates and elevation"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-01
---

# Exact timezone lookup, and accurate place coordinates and elevation

## 1. Question

Two core capabilities need evidence before their architecture pages and ADRs are written.

**A. Exact IANA timezone lookup, fast and offline.** The plan has five parts:

- boundaries from timezone-boundary-builder (TBB), using the full-IANA "with oceans" variant (ODbL);
- rules from tzdb;
- an adaptive Morton quadtree ("region index"). Leaves wholly inside one zone store only the zone ID. Border leaves store the edge fragments that touch the cell, at full source resolution, and test them with robust predicates;
- results identical to the source polygons, about 100–200 ns per lookup, a compact size, and a world tz pack that works on its own;
- an optional embedded compiled tzdb for offsets and DST.

The lite build is CC-BY, so it cannot use ODbL boundaries. It derives the zone from admin polygons plus the tzdb zone mapping. We need to know:

- the current TBB release, its variants and its sizes;
- how the existing libraries (tzf-rs, timezonefinder, node-geo-tz, S2 `ShapeIndex`, Android/ICU) trade size, speed and exactness;
- how tzdb releases, aliases and `zone1970.tab` work;
- which Rust tzdb crate to use;
- how to prove exactness, and which pitfalls matter.

**B. Accurate coordinates and elevation for places.** Accuracy is the project's top priority. The target for now is P90 vertical error ≤ 50 m, and tighter is better. The plan has these parts:

- a consensus point: a weighted geometric median over sources, detection of coordinates rounded to arc-minutes, and snapping to built-up density peaks;
- DEM sampling: Copernicus GLO-30 as primary, with NASADEM/AW3D30 for comparison;
- an uncertainty per record;
- QA checks: reported height vs DEM, and consistency with neighbours;
- snapping peaks to the DEM local maximum, and using official surveyed heights;
- a defined vertical datum.

We need evidence on DEM accuracy in high relief, the geoid conversion, the prior art for gazetteer coordinate quality, the robustness of the consensus method, and concrete QA tolerances.

This note feeds the timezone-engine and region-index architecture pages, the tz pack format, the elevation/coordinate pipeline in `05-data/`, the accuracy gates, and the licence ADR for the lite build.

## 2. Summary and recommendation

### A. Timezone

1. **Data.** Use TBB release **2026f** (published 2026-11-01), `timezones-with-oceans.geojson.zip`: 56.2 MB zipped, 444 zone IDs, 1,356 outer rings, 792 holes. The rings hold about 8.03 M boundary vertices plus 0.26 M hole vertices; the counts are from timezonefinder's 2026f data report. Pair each TBB release with the tzdb release of the same name (tzdb 2026f, released 2026-10-27). Gate: every TBB zone ID resolves in the bundled tzdb.
2. **Index design: confirm the plan, with three changes.**
   - (a) **Store original edges, not clipped fragments.** A border leaf lists the source edges that intersect the closed cell, with their original endpoints. This is what S2's `ShapeIndex` does ("the edges themselves are not clipped"). Clipping creates new, rounded vertices, so it cannot be exact.
   - (b) **Answer border cells with the S2 method.** Store, per leaf, the zone set at a reference point (the cell centre, placed on a half-unit offset so it never coincides with a source vertex). Count crossings of the segment from that point to the query against the leaf's edges. Each edge is labelled with the zone(s) whose boundary it belongs to, and each crossing toggles those zones. This handles holes, nested enclaves and source overlaps with orientation tests alone.
   - (c) **Use integer coordinates and integer predicates.** Store vertices as `i32` at 1e-7° (about 1.1 cm). TBB publishes 6 decimals, so this is lossless. Quantise queries to the same grid, and evaluate orientations exactly in `i128`. The predicates are then exact and identical on every platform, with no floating-point filter needed. Degenerate cases (collinear, or exactly on a boundary) follow one documented symbolic-perturbation rule.
3. **Expected size and speed (estimates, to be measured).** The exact world pack, with oceans, is about **15–25 MB**. The estimate assumes about 8.3 M vertices, delta and varint coding at roughly 2 bytes per coordinate, each shared border stored once with left/right zone labels, and a leaf index of a few MB. tzf-rs's full-precision `.tzb` is about 14 MB, and timezonefinder's unsimplified int32 data is 31.4 MB, so the range is plausible. Speed: interior-leaf hits should take tens of ns, and border leaves with ≤ 16–32 edges about 100–200 ns. The perf gate in §6 decides.
4. **Rules.** Do not depend on a crate's bundled tzdb version. They lag:
   - `jiff-tzdb` 0.1.9 embeds 2026e, so it misses Cuba's 2026-11-01 change in 2026f;
   - `chrono-tz` 0.10.4 embeds 2025b;
   - `tzdb_data` 0.2.5 embeds 2026b.

   Compile tzdb ourselves at pack build time into a concatenated TZif blob (about 200 KB, like `jiff-tzdb`'s 207 KB file). Load it through `jiff`'s TZif constructor (`jiff` 0.2.39).
5. **IDs.** Return the TBB zone ID unchanged. It keeps country-specific IDs such as `Europe/Amsterdam`, even where tzdb has made them links since 2022b. Offer canonicalisation through tzdb `backward` links as an option. Be aware that CLDR/ICU canonical IDs differ from IANA (for example `Asia/Calcutta` vs `Asia/Kolkata`).
6. **Lite build.** Single-zone countries are exact by construction. Multi-zone countries need a curated admin-1/admin-2 → zone table, and will be wrong wherever tz lines cut admin units, such as parts of the US, Canada, Mexico, Brazil and Australia. Publish the lite build's measured agreement with TBB, weighted by population and by area, as a gate output. Whether that measurement taints the CC-BY data is a licence question for the maintainer (**unverified**, §7).

### B. Coordinates and elevation

1. **The vertical budget depends mostly on horizontal accuracy.** Vertical error from position error is about σ_h · tan(slope). A GeoNames point rounded to the arc-minute has σ_h ≈ 535 m per axis (1852 m / √12). On a 20° Nepali hillside that alone gives about 195 m vertical, roughly 4× the P90 target. Copernicus DEM error itself is metres to low tens of metres. So the consensus point, and the rounding detector that down-weights coarse sources, are what make the 50 m target achievable. A better DEM is not.
2. **DEM.** Use Copernicus GLO-30 as primary. It ranked first among the 1″ global DEMs in eight high-relief LiDAR and ICESat-2 test areas (Guth & Geoffroy 2021). Its official global LE90 is 2.17 m against ICESat (validation report v3.0, 2020), but that figure deliberately filters out mountainous terrain. Its "Rock and Ice" ecoregion shows LE90 14.1 m and RMSE 9.5 m. Keep NASADEM and AW3D30 as comparison and fallback in void-fill areas. **Do not use FABDEM**: its data is CC BY-NC-SA 4.0. GEDTM30 (CC BY 4.0, 2025) is a lite-compatible bare-earth candidate to evaluate.
3. **Datum.** Everything is EGM2008 orthometric. Over Nepal the EGM2008 undulation runs from about −25 m in the north-west to about −66 m in the Terai, and over India from about −24 m (Ladakh) to −98 m (Kanyakumari). These are GeographicLib point evaluations, §3.B.4. Mixing ellipsoidal and orthometric heights is therefore a 25–100 m error, larger than the whole budget. SRTM, NASADEM and AW3D30 use EGM96. EGM96 differs from EGM2008 by about 2 m at Kathmandu and about 3.3 m in the Karakoram, so convert before comparing.
4. **Sampling.** Bilinear by default. Its interpolation error is about 1 m even at sharp ridges, against a DEM error of 5–25 m on slopes. Bicubic is measured in a decimation test, not assumed better.
5. **Uncertainty model.** Per record:

   σ_v² = σ_DEM(slope, land cover, fill)² + σ_relief(σ_h)² + σ_interp²

   σ_relief is the standard deviation of the DEM over the horizontal uncertainty disc. The QA check is **interval-based**: a reported height passes if it lies inside the 5–95 % DEM range over that disc, widened by 2·σ_DEM. A fixed tolerance would be meaningless when σ_h is large. Recommended tolerances are in §5.B.3.
6. **Peaks.** Snap to the DEM local maximum within max(90 m, 2σ_h). Report the official surveyed height as the elevation (for example Everest 8848.86 m, the 2020 Nepal/China survey), and keep the DEM height as a separate field. Expect GLO-30 to under-read sharp summits; the size of that bias is **unverified** and is measured in §5.B.5.

## 3. Findings

### A. Timezone

#### A.1 timezone-boundary-builder 2026f

Release assets, checked through the GitHub API on 2026-11-06:

| Asset | Size (bytes) | Zone IDs |
|---|---|---|
| `timezones-with-oceans.geojson.zip` | 56,156,331 | 444 |
| `timezones.geojson.zip` (land only) | 52,008,272 | 419 |
| `timezones-with-oceans-1970.geojson.zip` | 48,780,151 | 301 groups |
| `timezones-1970.geojson.zip` | 45,615,985 | — |
| `timezones-with-oceans-now.geojson.zip` | 26,768,042 | 62 groups |
| `timezones-now.geojson.zip` | 25,009,050 | — |
| shapefile equivalents | 39–89 MB | — |
| `input-data.zip` (OSM extracts) | 69,631,461 | — |

The 25 extra IDs in the oceans variant are `Etc/GMT`, `Etc/GMT±1…12`. They are 15° bands beyond territorial waters. The sign is POSIX-inverted: `Etc/GMT-5` means UTC+5.

- **1970 variant.** It merges zones with identical rules since 1970 (301 groups). For example, `Africa/Abidjan` absorbs `Africa/Accra`, `Atlantic/Reykjavik` and others. Its results are wrong for timestamps before 1970.
- **"now" variant.** It merges zones with identical rules from now on (62 groups). For example, `Europe/Moscow` absorbs `Europe/Istanbul`, `Asia/Baghdad` and `Asia/Riyadh`. It is valid only for current and future instants, and it can change with every tzdb release.
- **Our choice.** Neither merged variant fits a "full IANA" promise. Use the full variant. Derive the 1970 and "now" groupings at query time from tzdb, if wanted.
- **Geometry counts** (timezonefinder data report for 2026f, with oceans): 1,356 outer polygons, 792 holes, 100 polygons with holes, 8,031,751 boundary vertices and 262,577 hole vertices. Coordinates are published with 6 decimals, about 11 cm.
- **Cadence and source.** The 2026f release notes say: "Update to latest OSM data", recalculated 1970/now groupings, and a temporary workaround for a tzdb hack affecting Canadian zones (PR #217). TBB publishes within days of each tzdb release; 2026f came five days after tzdb 2026f. Boundaries come mostly from OSM, so they move between releases even when tzdb does not.

#### A.2 How existing libraries work

**tzf-rs 2.3.0** (MIT code; data ODbL via `tzf-dist`; v2 dropped protobuf):

- Two finders. `DefaultFinder` materialises polygons at load time and answers "most queries from the preindex tiles in ~100 ns". It falls back to point-in-polygon ray casting ("YStripes") for boundary cases. `EmbeddedFinder` queries the `.tzb` bytes in place, using about 4 MB plus a ~100 KB index, and takes "a few microseconds" at borders.
- The crates.io build embeds a **simplified** `lite.tzb` of about 4 MB. Its error is bounded: every simplified border stays within about 111 m of the full-precision border.
- The 100 % accurate `full.tzb` is about 14 MB. It is git-only, because it exceeds the crates.io package limit.
- Loading is costly, so the README recommends a global `LazyLock` finder.
- The `tzf-rel` crate on crates.io is still 0.0.2026-a, while `tzf-dist` git tags reach v0.0.2026-f.

Lesson: a preindex tile answer plus exact fallback is the right shape. Simplification breaks exactness, and crates.io's size limit forces an out-of-registry path for a ~14 MB pack.

**timezonefinder 9.2.0** (Python, MIT; data package `timezonefinder-data` 4.2026.6, ODbL). Its architecture page:

- Coordinates are scaled by 1e7 into `int32`, so the geometry path is integer-only.
- A 480 × 240 grid of 0.75° cells stores one `int16` per cell. A cell holds either a unique zone, a pointer to a 4 × 4 child block of 0.1875° cells, or a candidate polygon list. Lists are ordered so the scan can stop early, and "the last candidate is returned without a PIP test".
- Each candidate gets a bounding-box check. Holes are tested before the outer ring, and each ring is split into 128-vertex latitude blocks, which skips about 98 % of edge tests.
- There is no simplification: 31.4 MB of data, memory-mapped. Overlaps resolve to the zone with the smaller spherical area.
- Throughput is "hundreds of thousands of queries/s" in Python, about 2–10 µs per query.
- Against tzfpy (the binding of simplified tzf-rs), 47.5 % of sampled border locations differ at 10 m and 0.27 % at 100 m.

Lesson: a fixed root grid plus integer PIP is proven. Their remaining cost is the multi-polygon scan, which clipped per-cell edge lists remove.

**node-geo-tz 8.1.11** (npm, MIT; unpacked about 74 MB). Read from `src/find.ts`:

- A string-keyed quadtree (`a`–`d` per level) over the clamped box ±89.9999 / ±179.9999.
- A node is one of three things: a list of zone indexes (whole cell in those zones); a `{pos,len}` pointer into `geo.dat`, which holds geobuf-encoded clipped features tested with `@turf/boolean-point-in-polygon` in floating point; or absent, meaning sea, with zones computed from longitude.
- Features are lazy-loaded into an unexpiring cache. It returns an array, because zones can overlap.
- Pitfalls visible in the code: it clamps coordinates (lon 180 becomes 179.9999), so it is not exact at the edges of the world, and latitude 90 returns every ocean zone.

Lesson: this is the closest prior art to our design, but it is float-based, uses clipped geometry and JSON-ish nodes, and is far from 100 ns.

**S2 `ShapeIndex`** (Google S2, Apache-2.0):

- A linear quadtree of non-overlapping `S2CellId`s, one per non-empty leaf.
- Each cell holds `S2ClippedShape`s. Each records the shape id, the ids of the original edges that intersect the cell, and a `contains_center` flag.
- Point containment starts from the cell centre's known state and counts crossings to the point.
- `S2ContainsPointQuery` offers OPEN, SEMI_OPEN (default) and CLOSED vertex models. Under SEMI_OPEN, polygons that tile the plane give each shared vertex to exactly one of them.
- The default `max_edges_per_cell` is 10, and cells are padded for robustness. Both are from the S2 source, not the devguide page (**unverified** here).
- Caveat for us: S2 edges are geodesics. TBB GeoJSON edges are straight lines in lon/lat (RFC 7946 interpretation). Using S2 directly would therefore differ from the source polygons along long edges, such as straight-line borders in the Sahara or North America, and ocean band edges. We copy S2's index idea but keep planar lon/lat edges.

**Android GeoTZ / ICU:**

- AOSP's reference location time-zone provider (`packages/modules/GeoTZ`) builds `tzs2.dat` from TBB. Its pipeline goes GeoJSON → per-zone polygons → canonicalise (a TZ ID file with a "replacement threshold") → S2 cell unions at **level 12** → cell ranges → merge → a block file.
- S2 level 12 cells are about 2.5 km across, so the lookup is approximate near borders by design. It exists for detecting the device time zone, not for exactness.
- Android also detects time zones from telephony country, through a country → zone mapping (`tzlookup.xml`, **unverified** detail).
- ICU itself has **no geographic lookup**. It ships rules (`zoneinfo64`), CLDR metazones, the region of each zone, and CLDR canonical IDs. CLDR canonical IDs are frozen old names, for example `Asia/Calcutta`. ICU added an IANA-ID accessor in recent versions (ICU 74, `TimeZone::getIanaID`; **unverified**).

Lesson: our lite build is the "country plus admin" approach that Android uses for telephony. It is a known pattern, and it is approximate.

#### A.3 tzdb

- **Cadence.** 2023d (Dec 2023); 2024a, 2024b; 2025a, 2025b, 2025c; 2026a–2026f, six releases so far this year (2026f on 2026-10-27). So expect 2–6 releases a year, some with only days of notice, such as Cuba in 2026f.
- **Future-rule changes in recent releases:** Kazakhstan unified to +05 (2024a), Paraguay moved to permanent −03 (2024b), Cuba keeps −04 through 2027-11-07 (2026f). Rules need their own update path, independent of boundary releases.
- **Links and `backward`.** Deprecated names (`US/Eastern`, `Asia/Calcutta`) are links in `backward`; `chrono-tz` 0.10.4's copy has 274 `Link` lines. Since 2022b, tzdb has also moved zones whose data has matched another zone since 1970 into `backzone`, and links them by default (for example `Europe/Amsterdam` → `Europe/Brussels`). Building with `PACKRATDATA=backzone` restores their pre-1970 data. TBB keeps those zones as separate polygons.
- **Tab files** (counted 2026-11-06):

  | File | Entries | Content |
  |---|---|---|
  | `zone1970.tab` | 312 | Zones that differ since 1970; each line lists the countries using it and an ISO 6709 point |
  | `zone.tab` | 418 | Legacy; one country per line |
  | `zonenow.tab` | 91 | Added in 2023d; one zone per distinct present/future timekeeping, per region |

  For the lite build, `zone.tab` and `zone1970.tab` give the country → candidate zones map and a free-text region comment, for example "Mountain (most areas)". The comment is human-readable only, so admin polygons still need a curated mapping.

#### A.4 Rust tzdb crates

Checked on crates.io on 2026-11-06:

| Crate | Version | Embedded tzdb | Licence | Notes |
|---|---|---|---|---|
| `jiff` | 0.2.39 (2026-11-02) | via `jiff-tzdb` | Unlicense OR MIT | Reads system zoneinfo by default; bundles on request; can build a `TimeZone` from TZif bytes. Best API for us. |
| `jiff-tzdb` | 0.1.9 (2026-10-06) | **2026e** (`VERSION`), 206,788-byte concatenated TZif | Unlicense OR MIT | Lags 2026f |
| `chrono-tz` | 0.10.4 (2025-07-11) | **2025b** | MIT OR Apache-2.0 | Compiled into Rust code; stale |
| `tzdb` / `tzdb_data` | 0.7.3 / 0.2.5 (2026-05-18) | **2026b** | Apache-2.0; MIT-0/MIT/Apache-2.0 | Static data for `tz-rs` |
| `tz-rs` | 0.7.3 (2026-01-29) | none | MIT OR Apache-2.0 | TZif and POSIX TZ engine |

Conclusion: rely on `jiff` as the engine and supply our own TZif bundle, built by `zic` from the tzdb release matched to TBB. Whether `jiff-tzdb` is compiled with `backzone` is **unverified**. Our own build decides it explicitly: recommend `backzone`, so that `Europe/Amsterdam` keeps its own history, consistent with TBB's separate polygons.

#### A.5 Pitfalls

- **Disputed areas.** TBB follows OSM's de facto boundaries, for example Crimea (`Europe/Simferopol`), Kashmir/Aksai Chin, and Western Sahara (`Africa/El_Aaiun`). The zone returned is a political statement, so this belongs in the perspectives policy (`05-data/`).
- **Overlaps and gaps.** Source polygons can overlap; geo-tz returns arrays and timezonefinder prefers the smaller area. The API needs both `zone_at` (one ID, documented tie-break) and `zones_at` (all).
- **Holes and nested enclaves.** There are 792 holes. Hard cases:
  - Madha (Oman, `Asia/Muscat`) inside the UAE (`Asia/Dubai`), with Nahwa (UAE) inside Madha;
  - the Navajo Nation (`America/Denver`) inside Arizona (`America/Phoenix`), with the Hopi reservation back in Phoenix;
  - Büsingen (`Europe/Busingen`).

  All of these belong in the fixture set.
- **Dateline and edges of the world.** -180 and 180 are the same meridian and must return the same answer; geo-tz clamps instead. Normalise longitude into one half-open interval before quantising. The `Etc/GMT-12` and `Etc/GMT+12` bands meet at 180°, and Kiribati (`Pacific/Kiritimati`, UTC+14) sits near it. Geometry crossing the antimeridian is split in the source.
- **Poles.** At latitude ±90 every meridian meets. Define a deterministic answer: `Etc/GMT` at the poles, or the zone at a fixed reference longitude.
- **Antarctica.** Station zones (`Antarctica/Troll`, `McMurdo`, `Casey`, …) coexist with ocean bands. Which zone covers the interior ice in the with-oceans file is **unverified**. Test against the source, not intuition.
- **Ocean zones.** Land zones extend only to territorial waters. "At sea" is `Etc/GMT±N` by nautical convention, not a legal time. Expose a `land_only` flag, as timezonefinder does with `timezone_at_land`.
- **Boundary semantics.** Vertices lie on the 1e-6° grid and queries can fall exactly on edges. Decide once: a symbolic-perturbation rule for `zone_at`, and "on boundary: both zones" for `zones_at`.
- **Planar vs geodesic edges.** See S2 above. The oracle must use planar lon/lat edges.

### B. Coordinates and elevation

#### B.1 DEM accuracy, especially in high relief

| Evidence | DEMs | Reference | Result |
|---|---|---|---|
| Copernicus DEM Validation Report v3.0 (Airbus, 2020-11-09) | GLO-30 | ICESat GLAS, filtered (water removed; **standard-deviation filter "to eliminate ICESat points collected in mountainous terrain"**) | Global (15,050 1° cells): mean −1.29 m, σ 0.85 m, RMSE 1.68 m, LE90 2.17 m. By ecoregion: Montane grasslands LE90 1.87 m; Temperate conifer forest 2.80; Tundra 2.83; **Rock and Ice LE90 14.14 m, RMSE 9.50 m** (only 3 cells). The headline numbers are not valid for the Himalaya. |
| Guth & Geoffroy 2021, *Trans. GIS* 25(5) | GLO-30, ALOS AW3D30, ASTER, NASADEM, SRTM | Dense airborne LiDAR and ICESat-2, 8 high-relief areas | Copernicus best in all areas, slopes and vegetation types. DEMs sit "within the canopy", between DSM and DTM. NASADEM ≈ SRTM; ASTER worst. |
| Chen et al. 2022, *The Cryosphere* 16:197 | NASADEM, AW3D30, SRTM-GL1, TanDEM-X 90, SRTM v4.1, MERIT (no Copernicus) | ICESat-2 ATL06, about 3.5 M on-glacier points, Tibetan Plateau | RMSE: AW3D30 11.3 m, NASADEM 12.6, SRTM-GL1 13.5, TanDEM-X 15.1 (bias 0.1), MERIT 15.6, SRTM v4.1 17.0. Error spread grows by about 1.1–1.3 m per degree of slope. North-facing slopes are worse (SRTM-GL1 median from −5 m on south-facing to +5 m on north-facing). Glacier surfaces also change over time. |
| Global DEMs in China (2022, *IJDE*, doi 10.1080/17538947.2022.2094002) | Copernicus, NASADEM, AW3D30 | ICESat-2 ATL08 | RMSE 6.73 / 6.59 / 6.63 m. Accuracy falls with elevation and slope, not with aspect. AW3D30 overestimates, NASADEM underestimates. |
| HMA multi-level assessment (secondary snippet, **unverified**) | FABDEM, Copernicus, others | — | Copernicus and FABDEM showed the strongest slope effect on derived slope accuracy |
| Hawker et al. 2022, *ERL* 17:024016 (FABDEM) | Copernicus vs FABDEM | LiDAR DTM | Mean absolute error in forests 5.15 → 2.88 m; built-up 1.61 → 1.12 m. Remaining biases on steep terrain. **Data CC BY-NC-SA 4.0.** |
| Ho et al. 2025, *PeerJ* 13:e19673 (GEDTM30) | GLO-30, AW3D30, FABDEM, GEDTM30 | LiDAR DTM | GLO-30 RMSE/ME: tree cover >50 % 5.14/−2.94 m; 10–50 % 3.19/−0.73; built-up 4.06/−3.06. AW3D30 worse in each class. GEDTM30 is CC BY 4.0. |

- **No Nepal-specific, Copernicus-inclusive, ICESat-2-based study was found** in this pass, so it is an open item. The nearest evidence is the Tibetan Plateau, Chinese mountains, and Guth's eight high-relief areas.
- **ICESat-2 as a reference.** ATL08 terrain height uncertainty is about 0.2 m on plains and about 2 m in mountains (secondary source, **unverified**). That is acceptable against a 50 m budget.

**DSM vs DTM.** GLO-30 (X-band InSAR) and AW3D30 (optical stereo) are surface models. SRTM/NASADEM (C-band) penetrates partly. In forest, expect the DEM to read several metres above the ground: a mean absolute error of about 5 m (Hawker), and up to 10–20 m under tall closed canopy (**unverified** upper end). In built-up areas expect 1–4 m. Both are small against the slope term, but they must enter σ_DEM.

**Void fill.** GLO-30 is resampled from WorldDEM's 0.4″ to 1″. It ships auxiliary layers: a height error mask (HEM, per-pixel interferometric height error), an editing mask (EDM), a fill mask (FLM, which records the source used for filling) and a water-body mask (WBM). Fills use other DEMs: ASTER GDEM, SRTM, AW3D and national DEMs (ESSD 2025 summary). Radar layover and shadow make fills common on steep Himalayan slopes. The share of filled pixels in Nepal is **unverified**. Whether the public COG distributions carry HEM and FLM, or only the DGED package does, is **unverified**. Recommendation: read FLM and HEM wherever available, and inflate σ_DEM to the fill source's class.

**Grid conventions.** GLO-30 uses 1″ latitude spacing. Longitude spacing widens beyond 50° latitude (1.5″, 2″, 3″, 5″, 10″ in latitude bands; **unverified** exact band edges). Tiles are point-registered. Both must be handled in the sampler and tested at tile seams.

#### B.2 Comparison DEMs

| DEM | Basis | Datum | Licence | Use |
|---|---|---|---|---|
| Copernicus GLO-30 | TanDEM-X InSAR, 2011–2015 | EGM2008 | Copernicus DEM licence: free, redistribution of derived values allowed with attribution. Armenia and Azerbaijan are GLO-90 only (**unverified**, check current terms) | Primary |
| NASADEM | SRTM 2000 reprocessed, voids filled (ASTER GDEM v3 and others) | EGM96 | Public domain (NASA) | Second opinion; independent sensor and date |
| AW3D30 v4.x | PRISM stereo 2006–2011 | EGM96 | Free with attribution (JAXA; **unverified** current version) | Steep-slope comparison; strong on glaciers (Chen 2022) |
| SRTM GL1 | C-band 2000 | EGM96 | Public domain | Superseded by NASADEM |
| FABDEM | Copernicus with forests and buildings removed | EGM2008 | CC BY-NC-SA 4.0 | **Excluded** (non-commercial) |
| GEDTM30 | Ensemble DTM | EGM2008 (**unverified**) | CC BY 4.0 | Candidate bare-earth layer, including for lite |

#### B.3 Sampling: bilinear vs bicubic

- The literature mostly favours bicubic for resampling and downscaling. On very rough terrain the methods converge (Vietnamese resampling studies; Shi et al.). One Japanese study preferred bilinear. No study was found that reports point-elevation RMSE for both methods on the same mountain dataset.
- Analytically, bilinear error is ≤ (h²/8)·(|f_xx| + |f_yy|). With h = 30 m and a sharp ridge of 100 m curvature radius, that is about 1.1 m. Bicubic (Keys/Catmull-Rom) can overshoot at summits and cliffs.
- Bilinear never exceeds the four corner values, so it under-reads summits. That is acceptable, because peaks take their official heights.

#### B.4 Geoid (EGM2008)

Point values from GeographicLib GeoidEval, 2026-11-06:

| Point | EGM2008 N (m) | EGM96 N (m) |
|---|---|---|
| Everest summit 27.988 N 86.925 E | −28.50 | −28.74 |
| Kathmandu 27.7 N 85.3 E | −48.35 | −50.35 |
| Far-west Nepal hills 29.6 N 82.0 E | −27.65 | — |
| NW Nepal 30.2 N 81.0 E | −26.69 | — |
| Terai 26.9 N 85.0 E | −65.92 | — |
| Terai 27.2 N 84.5 E | −64.64 | — |
| SE Nepal 26.35 N 88.15 E | −56.61 | — |
| Ladakh 34.0 N 77.6 E | −24.12 | −23.78 |
| Karakoram 35.88 N 76.51 E | −25.71 | −22.41 |
| Mumbai 19.0 N 72.8 E | −67.75 | −68.49 |
| Bengaluru 12.97 N 77.59 E | −86.16 | −86.42 |
| Kanyakumari 8.08 N 77.55 E | −98.27 | −98.60 |

- **Ranges.** Nepal runs from about −25 to −66 m, with a gradient of about 0.2 m/km from north to south. India runs from about −24 to −98 m. The Indian Ocean geoid low, south of Sri Lanka, is beyond −100 m.
- **Conversion.** Ellipsoidal h = orthometric H + N.
  - GNSS and ICESat-2 heights are ellipsoidal: convert with H = h − N.
  - EGM96-based DEMs: H₂₀₀₈ = H₉₆ + N₉₆ − N₂₀₀₈, which is up to a few metres.
  - National datums, such as Nepal's or India's MSL-based heights, differ from EGM2008 by metre-level amounts (**unverified** magnitude).
- **Grid sizes and errors** (GeographicLib docs; 16-bit PGM, 3 mm quantum):

  | Grid | Size | Bilinear max / RMS | Cubic max / RMS |
  |---|---|---|---|
  | 5′ | 19 MB | 0.478 m / 12 mm | 0.294 m / 4.5 mm |
  | 2.5′ | 75 MB | 0.135 m / 3.2 mm | 0.031 m / 0.8 mm |
  | 1′ | 470 MB | 0.025 m / 0.8 mm | 0.0022 m / 0.7 mm |

- **Cropped grid sizes** (computed, 16-bit):

  | Crop | 2.5′ | 1′ |
  |---|---|---|
  | Nepal (26–31 N, 80–89 E) | 121 × 217 ≈ 52 KB | 301 × 541 ≈ 326 KB |
  | South Asia (6–37 N, 68–98 E) | 745 × 721 ≈ 1.07 MB | 1861 × 1801 ≈ 6.7 MB |

  2.5′ bilinear (max 0.135 m) is more than sufficient.

#### B.5 Gazetteer coordinate quality: prior art

- **GeoNames.** Ahlers 2013 (GIR '13, doi 10.1145/2533888.2533938) classifies errors as: loss of coordinate granularity (rounded coordinates, for example 14.08333, −87.18333, which is 14°05′, 87°11′), wrong feature codes, near-duplicates, and places outside their country. His quantitative distance errors were not retrieved (**unverified**).
- **Wikidata.** `GlobeCoordinateValue` carries an explicit `precision` in degrees (Wikibase data model). Values imported from DMS infoboxes commonly carry 1/60 (arc-minute) or 1/3600 (arc-second). A missing or invalid precision defaults to 1/3600 in the formatter. So the precision field is a direct input to σ_h, but it can overstate the true precision.
- **OSM.** The `place=*` node goes "roughly at its centre", and for a populated place with a central town hall, square or similar, "preferably there" (wiki Key:place). Place areas have "no particular meaning". So the OSM node is a cultural centre, not a density centroid. For towns and cities that is the right answer, and density snapping must not override it.
- **Rounding detection.** A value is suspect when both latitude and longitude are within ε of a multiple of 1/60° (ε = 1e-5 for 5-decimal GeoNames, where 1/60 is stored as 0.01667). The chance of this happening by accident is about (2ε·60)² ≈ 1.4e-6 per point, so false positives are negligible. Check grids of 1/60, 1/120 (30″), 0.1, 0.01 and 0.001°. Implied σ_h per axis (uniform rounding) is step/√12:

  | Step | σ_h, latitude | σ_h, longitude at 28° N |
  |---|---|---|
  | 1′ | 535 m | 472 m |
  | 0.01° | 321 m | — |
  | 30″ | 267 m | — |
  | 1″ | 9 m | — |

#### B.6 Geometric median (Weiszfeld)

- The spatial median has a 50 % breakdown point (Lopuhaä & Rousseeuw 1991). Weiszfeld's iteration (1937) converges except when an iterate lands on a data point. Vardi & Zhang (2000, *PNAS*) give a modified iteration that converges in that case.
- **Compute it in a local tangent plane** (azimuthal equidistant around the initial estimate), not in raw degrees. Weight each source by 1/σ_h,source.
- With 2 sources the median is any point on the segment between them, so it is not robust. With 2 disagreeing sources, use source priority plus the larger σ_h.
- Breakdown only helps from 3 independent sources. GeoNames, Wikidata and OSM are often copies of each other, so de-duplicate sources that agree to within 1e-6° before voting.

#### B.7 Building footprints for settlement centres

- **Google Open Buildings v3** (inference May 2023): about 1.8 B detections over 58 M km², covering Africa, South and South-East Asia (India and Nepal included) and Latin America. Dual licence CC BY 4.0 / ODbL, so it is lite-compatible under CC BY. Detections are filtered at confidence ≥ 0.65. Per-S2-level-4 thresholds are published for 80/85/90 % precision; use the 90 % threshold. Imagery dates vary by place.
- **Overture buildings** (release 2026-10-21.0): ODbL, so it is **not lite-compatible**. It conflates OSM (top priority), Esri Community Maps, a few authoritative sets, and Microsoft, Google and East-Asia machine-learning footprints. Matching uses IoU > 0.5, with stable GERS IDs.
- **Method.** Mean-shift or KDE on footprint area (bandwidth about 150–300 m) inside the uncertainty disc. Snap only when all of these hold:
  - the place is a settlement class (village or hamlet; town only without an OSM node);
  - the peak is unique (second-peak density ratio < 0.8);
  - the peak lies inside the place's admin unit;
  - the snap distance ≤ 2.5σ_h.

  Snapping replaces σ_h with about half the KDE bandwidth (**to calibrate**).

#### B.8 Ground truth

- **ICESat-2 ATL08** terrain heights (NASA, public domain; ellipsoidal, so convert with EGM2008). Use for the DEM component, stratified by slope and land cover. ESA WorldCover (CC BY 4.0) gives the land-cover classes.
- **swisstopo swissNAMES3D** (open government data since 2021, attribution; **unverified** exact terms): 3D name points in high relief. It tests the horizontal and vertical error of the whole pipeline.
- **Kartverket place names** (Norway, CC BY 4.0, **unverified**): steep terrain.
- **USGS GNIS** (public domain) and **NGS datasheets**, which give surveyed NAVD88 heights. NAVD88 is about 0.5–1 m from EGM2008 (**unverified**).
- **Survey of Nepal / Survey of India trig points and benchmarks.** Their open availability is **unverified**; Survey of India is largely restricted. Official peak heights (for example Everest 8848.86 m, announced 2020-12-08) are citable facts.

## 4. Options compared

**Timezone lookup engines and data:**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| tzf-rs `DefaultFinder` + lite | ~100 ns typical; small (~4 MB) | Simplified; borders up to ~111 m off; not exact | MIT; data ODbL | 2.3.0 (2026-11-02) | Benchmark and differential reference only |
| tzf-rs + `full.tzb` | Exact source data; ~14 MB | Git-only data; border cases µs-level in `EmbeddedFinder`; heavy load time | MIT; ODbL | 2.3.0; tzf-dist v0.0.2026-f | Strongest baseline to beat on size |
| timezonefinder (Python) | Unsimplified, integer PIP, mmap, good docs and data report | Python; 31 MB; µs per query | MIT; data ODbL | 9.2.0; data 4.2026.6 | Oracle and design reference |
| node-geo-tz | Quadtree with exact leaves (our shape); returns all overlaps | Float, clipped geometry, clamps coordinates, ~74 MB, ms on first touch | MIT; data ODbL | 8.1.11 | Oracle; anti-patterns noted |
| S2 `ShapeIndex` (port) | Proven index; robust predicates; reference-point crossing method | Geodesic edges ≠ source; heavy dependency | Apache-2.0 | mature | Copy the design, not the code |
| Android GeoTZ `tzs2.dat` | Compact S2 ranges | Level-12 (~2.5 km) approximation; canonicalised IDs | Apache-2.0; data ODbL | AOSP main | Not suitable |
| **Own Morton quadtree, original edges, integer predicates** | Exact by construction; one dependency-free format; 100–200 ns target | Must be built and proven | Ours; data ODbL | — | **Recommended** |

**tzdb in Rust:**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `jiff` + our TZif bundle | Modern API; TZif constructor; we control the tzdb version | Must build `zic` output in our pipeline | Unlicense/MIT | 0.2.39 | **Recommended** |
| `jiff` + `jiff-tzdb` | Zero work | 2026e today; lags releases | Unlicense/MIT | 0.1.9 | Fallback |
| `chrono-tz` | Ubiquitous | 2025b embedded; compiled-in code; chrono API | MIT/Apache-2.0 | 0.10.4 | No |
| `tz-rs` + `tzdb_data` | `no_std` | 2026b | MIT/Apache-2.0 | 0.7.3 / 0.2.5 | `no_std` fallback only |

**DEMs:**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Copernicus GLO-30 | Best-ranked 1″ DEM; EGM2008; HEM/FLM masks | DSM; steep-slope radar artefacts; fills | Copernicus DEM licence | 2021/2022 releases | **Primary** |
| AW3D30 | Good on steep and glacier terrain | DSM; EGM96; older | JAXA free, attribution | v4.x (**unverified**) | Comparison |
| NASADEM | Independent epoch and sensor; public domain | Worse on steep slopes | PD | 2020 | Comparison |
| FABDEM | Bare earth | Non-commercial | CC BY-NC-SA 4.0 | v1.2 | Excluded |
| GEDTM30 | Bare earth; CC BY | New; mountain performance unknown | CC BY 4.0 | 2025 | Evaluate |

## 5. What we build ourselves, and why

### A. Region index for timezones

1. **Build.**
   - Parse the TBB GeoJSON and quantise to `i32` at 1e-7°. The source has 6 decimals, so this is exact; assert it.
   - Build a planar arrangement of rings. Store each distinct edge once with its zone labels, usually two zones, and one or three in overlaps and gaps.
   - Subdivide a lon/lat Morton quadtree from a fixed root grid. Use level 8, that is 256 × 256 cells of 1.40625° × 0.703125°, as a directly indexed array of 65,536 × 4 B = 256 KB.
   - Split a cell while it holds more than E edges (E around 16–32, to tune) and is above a minimum size.
   - A leaf is either a **uniform** zone ID (or zone set), or a **border** leaf holding its edge-id list plus the zone set at its reference point. The reference point state is computed at build time with the brute-force oracle.
2. **Query.**
   - Normalise and quantise the query, then compute the Morton code (BMI2 `pdep` where available).
   - Read the root grid, then descend the compact subtree (pointerless, implicit, or a sorted leaf-key array).
   - On a uniform leaf, return the zone.
   - On a border leaf, run exact segment-crossing tests from the reference point to the query against the leaf's edges. Use `i128` orientations with a symbolic-perturbation tie rule, toggle the labelled zones, and return the resulting set.
3. **Format.**
   - Edges are delta and zigzag-varint coded per leaf, with a shared vertex pool.
   - The index is mmap-friendly and little-endian, with a header carrying the TBB release, the tzdb release and checksums.
   - The tzdb TZif bundle is a separate section (about 200 KB), so the world tz pack works standalone.
4. **Measurements that must prove it:**
   - exactness: §6, gate G-TZ-EXACT;
   - speed: G-TZ-PERF;
   - size: G-TZ-SIZE, ≤ 25 MB exact world pack with oceans, which is to beat tzf-rs full's ~14 MB on a like-for-like basis or explain the gap;
   - cold open: mmap ≤ 1 ms, with no load-time polygon materialisation (tzf-rs's main cost).

### B. Coordinates and elevation pipeline

1. **Consensus point.**
   - Collect candidates (GeoNames, Wikidata, OSM, national gazetteers) and give each its own σ_h. σ_h comes from the precision class (rounding detector, Wikidata `precision`) and from a per-source prior learned on the ground-truth sets.
   - De-duplicate copies. Take the weighted geometric median (Vardi–Zhang Weiszfeld) in a local tangent plane.
   - Apply density snapping for eligible settlements (§3.B.7). For towns and cities, prefer the OSM `place` node when it is within 2.5σ of the median.
   - Output (lat, lon, σ_h, method).
2. **Elevation.**
   - Bilinear GLO-30 at the consensus point. Convert all external heights to EGM2008 orthometric.
   - Compute σ_v = sqrt(σ_DEM² + σ_relief² + σ_interp²), where:
     - σ_relief is the standard deviation of GLO-30 over the disc of radius σ_h. This captures non-planar terrain better than σ_h·tan(slope).
     - σ_DEM is a prior: σ_DEM = max(HEM_pixel, 2 m + 0.5 m·slope°) + land-cover term (forest 5 m, built-up 3 m, snow/ice 10 m) + fill term (filled pixel: the fill source's class, ASTER-like about 10–15 m). Priors are from §3.B.1, **to calibrate** on ICESat-2 ATL08.
     - σ_interp is 1 m.
   - Report H, σ_v, and the 90 % interval.
3. **QA checks.**

   **Reported vs DEM, interval form.** The check passes if H_rep ∈ [q05 − 2σ_DEM, q95 + 2σ_DEM], where q05 and q95 are DEM quantiles over the σ_h disc (minimum disc radius 45 m). Equivalent fixed tolerances for a well-located point (σ_h = 30 m), with tolerance T = max(15 m, 2.5·σ_v):

   | Slope | σ_DEM | 30·tanθ | σ_v | Tolerance T |
   |---|---|---|---|---|
   | < 5° | 4.5 m | 2.6 m | 5.2 m | **15 m** |
   | 5–15° | 9.5 m | 8.0 m | 12.4 m | **31 m** |
   | 15–30° | 17 m | 17.3 m | 24.3 m | **61 m** |
   | 30–45° | 24.5 m | 30 m | 38.7 m | **97 m**, flag `steep` |

   At σ_h = 535 m (arc-minute) on a 20° slope the vertical term alone is about 195 m. In that case the check cannot validate a height. It can only falsify gross errors, and the record is flagged `coarse_position`.

   **Systematic-error detectors**, run per source and per country on the median residual:
   - residual ≈ +N (ellipsoidal heights mislabelled as orthometric, −25 to −66 m in Nepal);
   - H_rep / H_DEM ≈ 3.2808 (feet);
   - residual ≈ EGM96 − EGM2008 (old datum);
   - lat/lon swapped (DEM residual collapses after the swap).

   **Neighbour consistency.** Within 25 km and the same source, flag |r_i − median(r_nbrs)| > 3 · 1.4826 · MAD(r_nbrs), where r = H_rep − H_DEM.
4. **Peaks.**
   - Search the DEM local maximum within R = max(90 m, 2σ_h). Require it to be the unique maximum in R and to have at least 30 m relief to R's edge.
   - Move the point there. Set σ_h to half a pixel diagonal plus the DEM's horizontal error.
   - Elevation = the official surveyed height if available and |H_off − H_DEM,max| < 50 m + 0.01·H. Otherwise flag. Keep `elevation_dem` as well.
5. **Measurements that must prove it:**
   - G-ELEV-P90: P90 |ΔH| ≤ 50 m on a held-out high-relief set (swissNAMES3D, Norway, a Nepal ATL08-matched set), reported per slope class and land cover;
   - G-ELEV-CAL: 90 % interval coverage between 87 and 93 %;
   - G-COORD: P90 horizontal error per place class, against swissNAMES3D and GNIS;
   - G-INTERP: decimation test. Downsample GLO-30 to 2″ and predict the dropped 1″ posts. Report bilinear vs bicubic RMSE by slope, and keep bilinear unless bicubic wins by > 10 % on slopes > 20°;
   - G-PEAK: GLO-30 summit under-read vs official heights on ≥ 200 surveyed peaks.

## 6. Impact on the plan

- **New ADR: tz region index format.** Original edges (not clipped), integer 1e-7° coordinates, `i128` exact predicates, reference-point crossing, edge zone labels, the root grid plus Morton subtree, and the boundary tie rule.
- **New ADR: tzdb source.** Our own `zic` TZif bundle loaded via `jiff`. Pin a TBB/tzdb release pair. Build with `backzone`. Return TBB IDs and offer canonicalisation via `backward`.
- **New ADR (licence): lite timezone.** Admin + `zone1970.tab` + curated table. Measured agreement is published. Open legal question on evaluating against ODbL data.
- **Architecture pages:** tz engine; region index; elevation and coordinate pipeline; uncertainty model (§5.B.2), owned by the schema page in `05-data/`.
- **Gates** (to `06-engineering/` and the generated `ACCURACY.md`, `SIZES.md` and `BENCHMARKS.md`):
  - **G-TZ-EXACT:** zero disagreements with the brute-force integer oracle on these sets:
    1. 10⁸ uniform-on-sphere points;
    2. border stress: for every edge, the midpoint offset perpendicular by ±1e-7, 1e-6, 1e-5, 1e-4 and 1e-3°, plus every vertex ±1e-7° in 8 directions (about 66 M points);
    3. points exactly on every Morton cell edge and corner at every level;
    4. fixtures: poles, ±180, (0,0), the enclave list in §3.A.5, Antarctic stations, Kiribati;
    5. all GeoNames populated places.

    In addition: no disagreements with a second, independently written oracle (GEOS/shapely `covers` on the f64 GeoJSON) beyond 1e-6° of an edge. Cross-checks against timezonefinder 9.2.0 and geo-tz 8.1.11 on the same TBB release, with every disagreement classified (overlap tie-break, clamp, boundary). Property tests: longitude wrap invariance, and quantisation idempotence.
  - **G-TZ-PERF:** p50 ≤ 150 ns and p99 ≤ 500 ns single-threaded, on uniform and population-weighted query mixes, on the reference machine. tzf-rs `DefaultFinder` lite and full are reported alongside.
  - **G-TZ-SIZE:** exact world pack ≤ 25 MB.
  - **G-TZ-IDS:** every boundary ID resolves in the bundled tzdb. The TBB and tzdb release names match.
  - **G-ELEV-P90, G-ELEV-CAL, G-COORD, G-INTERP, G-PEAK** (§5.B.5).
- **Roadmap:** a spike to build the index on 2026f and measure size, speed and exactness before format freeze. A data task to assemble the ground-truth sets and licences. A recurring task to rebuild the tz pack on each tzdb/TBB release, at 2–6 per year.
- **Data sizes summary:**

  | Item | Size |
  |---|---|
  | TBB with-oceans download | 56 MB zip |
  | Exact tz pack | ≈ 15–25 MB (estimate) |
  | tzdb TZif bundle | ≈ 0.2 MB |
  | EGM2008 world grid | 5′ 19 MB, 2.5′ 75 MB |
  | EGM2008 crop, Nepal | 2.5′ 52 KB |
  | EGM2008 crop, South Asia | 2.5′ 1.07 MB |
  | Per-place elevation | `i16` H + `u8` σ class ≈ 3 B per record (raw) |

  DEMs are used at build time only and are never shipped.

## 7. Risks and open items

- **No Nepal-specific GLO-30 validation found.** Our own ATL08-based Nepal evaluation is the first deliverable of the elevation track. Glacier change since 2011–2015 adds a time-dependent error on ice.
- **The P90 ≤ 50 m target fails for coarse-position records on slopes.** It needs ≥ 3 independent sources or density snapping. Report the share of records that cannot meet it, rather than hiding them.
- **Licences.**
  - Whether evaluating the CC-BY lite tz against ODbL TBB creates a derivative obligation is **unverified**; ask counsel.
  - Copernicus DEM licence terms for redistributing derived per-place heights: **verify**.
  - Overture buildings are ODbL, so not usable for lite.
  - FABDEM is excluded.
- **TBB boundary churn from OSM edits,** including vandalism. Diff consecutive releases and review large area changes before shipping.
- **Release lag in dependencies** (jiff-tzdb 2026e, chrono-tz 2025b). Mitigated by our own TZif bundle; G-TZ-IDS catches mismatches.
- **Exactness is defined on quantised input.** Queries are rounded to 1e-7° (1.1 cm). Document that "identical to the source polygons" holds for that definition, and that points within about 1 cm of a border follow the tie rule.
- **Size estimate is unmeasured.** If the arrangement (shared-edge) step is hard to make robust on 8 M vertices, per-polygon storage roughly doubles vertex bytes. Fall back to per-polygon storage and measure.
- **Unverified items** listed in §3:
  - S2 `max_edges_per_cell` default;
  - ICU `getIanaID`;
  - `jiff-tzdb` `backzone`;
  - Antarctic interior coverage;
  - the GLO-30 longitude-spacing bands, and HEM/FLM in the public COGs;
  - AW3D30 version;
  - GEDTM30 datum;
  - ATL08 mountain uncertainty;
  - Ahlers' numbers;
  - the swissNAMES3D and Kartverket terms;
  - NAVD88 offset;
  - national datum offsets.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| timezone-boundary-builder | 2026f (published 2026-11-01) | 2026-11-06 | GitHub releases API |
| tzdb | 2026f (2026-10-27) | 2026-11-06 | data.iana.org NEWS |
| `zone1970.tab` / `zone.tab` / `zonenow.tab` | 312 / 418 / 91 entries | 2026-11-06 | data.iana.org |
| `jiff` | 0.2.39 | 2026-11-06 | crates.io API |
| `jiff-tzdb` | 0.1.9, embeds 2026e | 2026-11-06 | crates.io (crate source `tzname.rs`) |
| `chrono-tz` | 0.10.4, embeds 2025b | 2026-11-06 | crates.io (crate `tz/NEWS`) |
| `tzdb` / `tzdb_data` | 0.7.3 / 0.2.5, embeds 2026b | 2026-11-06 | crates.io |
| `tz-rs` | 0.7.3 | 2026-11-06 | crates.io |
| `tzf-rs` / `tzf-rel` | 2.3.0 / 0.0.2026-a | 2026-11-06 | crates.io |
| `tzf-dist` | v0.0.2026-f (git tag) | 2026-11-06 | GitHub tags API |
| timezonefinder / timezonefinder-data | 9.2.0 / 4.2026.6 | 2026-11-06 | PyPI |
| tzfpy | 2.1.2 | 2026-11-06 | PyPI |
| geo-tz | 8.1.11 | 2026-11-06 | npm registry |
| `robust` (Shewchuk predicates) | 1.2.0 | 2026-11-06 | crates.io |
| `geo` | 0.33.1 | 2026-11-06 | crates.io |
| `i_overlay` | 9.1.0 | 2026-11-06 | crates.io |
| `geo-index` | 0.4.0 | 2026-11-06 | crates.io |
| `geographiclib-rs` | 0.2.7 | 2026-11-06 | crates.io |
| `egm2008` crate | 0.1.0 (2023; grid unknown) | 2026-11-06 | crates.io |
| Android GeoTZ pipeline | `main`, S2 level 12 | 2026-11-06 | android.googlesource.com |
| Copernicus DEM Validation Report | v3.0 (2020-11-09) | 2026-11-06 | Copernicus portal PDF |
| Google Open Buildings | v3 (May 2023 inference) | 2026-11-06 | sites.research.google |
| Overture buildings | 2026-10-21.0 | 2026-11-06 | docs.overturemaps.org |
| EGM2008 values | GeographicLib GeoidEval | 2026-11-06 | geographiclib.sourceforge.io |

## 9. Sources

- [timezone-boundary-builder releases](https://github.com/evansiroky/timezone-boundary-builder/releases), E. Siroky, release 2026f, accessed 2026-11-06.
- [tz database NEWS](https://data.iana.org/time-zones/tzdb/NEWS), IANA, 2026f 2026-10-27, accessed 2026-11-06.
- [zone1970.tab](https://data.iana.org/time-zones/tzdb/zone1970.tab), [zonenow.tab](https://data.iana.org/time-zones/tzdb/zonenow.tab), IANA, accessed 2026-11-06.
- [tzf-rs README](https://crates.io/crates/tzf-rs), ringsaturn, v2.3.0, accessed 2026-11-06.
- [timezonefinder architecture](https://timezonefinder.readthedocs.io/en/latest/architecture.html), [alternatives](https://timezonefinder.readthedocs.io/en/latest/alternatives.html), [data report](https://timezonefinder.readthedocs.io/en/latest/data_report.html), J. Kissinger, accessed 2026-11-06.
- [node-geo-tz](https://github.com/evansiroky/node-geo-tz) and [src/find.ts](https://github.com/evansiroky/node-geo-tz/blob/master/src/find.ts), accessed 2026-11-06.
- [S2ShapeIndex developer guide](http://s2geometry.io/devguide/s2shapeindex), Google, accessed 2026-11-06.
- [AOSP GeoTZ module](https://android.googlesource.com/platform/packages/modules/GeoTZ/+/refs/heads/main/) and [run-data-pipeline.sh](https://android.googlesource.com/platform/packages/modules/GeoTZ/+/refs/heads/main/run-data-pipeline.sh), accessed 2026-11-06.
- [RFC 7946, The GeoJSON Format](https://www.rfc-editor.org/rfc/rfc7946), IETF, 2016.
- [crates.io API](https://crates.io/api/v1/crates/jiff) for `jiff`, `jiff-tzdb`, `chrono-tz`, `tzdb`, `tzdb_data`, `tz-rs`, `tzf-rs`, `robust`, `geo`, `i_overlay`, `geo-index`, `geographiclib-rs`, `egm2008`, accessed 2026-11-06.
- [Copernicus DEM Validation Report v3.0](https://s3.waw3-1.cloudferro.com/swift/v1/portal_uploads_prod/GEO1988-CopernicusDEM-RP-001_ValidationReport_I3.0_08.2024.pdf), Airbus Defence and Space, 2020-11-09, accessed 2026-11-06.
- Guth, P. L. & Geoffroy, T. M., [LiDAR point cloud and ICESat-2 evaluation of 1 second global DEMs: Copernicus wins](https://icesat-2.gsfc.nasa.gov/pubs/2021/lidar-point-cloud-and-icesat-2-evaluation-1-second-global-digital-elevation-models), *Transactions in GIS* 25(5):2245–2261, 2021, doi 10.1111/tgis.12825.
- Bielski, C. et al., [Novel approach for ranking DEMs: Copernicus DEM improves one arc second open global topography](https://arxiv.org/abs/2302.08425), arXiv / IEEE TGRS, 2024.
- Chen, W. et al., [Towards ice-thickness inversion: an evaluation of global DEMs in the glacierized Tibetan Plateau](https://tc.copernicus.org/articles/16/197/2022/), *The Cryosphere* 16:197–218, 2022.
- [Global DEMs vary from one to another: Copernicus, NASA and AW3D30 on selected terrains of China using ICESat-2](https://www.tandfonline.com/doi/full/10.1080/17538947.2022.2094002), *Int. J. Digital Earth*, 2022.
- Hawker, L. et al., [A 30 m global map of elevation with forests and buildings removed](https://iopscience.iop.org/article/10.1088/1748-9326/ac4d4f), *Environ. Res. Lett.* 17:024016, 2022.
- Ho, Y.-F. et al., [GEDTM30: global ensemble digital terrain model at 30 m](https://pmc.ncbi.nlm.nih.gov/articles/PMC12296579/), *PeerJ* 13:e19673, 2025.
- [Multi-source DEM accuracy evaluation based on ICESat-2 in Qinghai-Tibet Plateau](https://www.tandfonline.com/doi/full/10.1080/17538947.2023.2297843), *Int. J. Digital Earth*, 2023 (not read: paywalled).
- [ESSD 17:205, 2025](https://essd.copernicus.org/articles/17/205/2025/) (summary of GLO-30 void-fill sources), accessed 2026-11-06.
- [GeographicLib: geoid height](https://geographiclib.sourceforge.io/C++/doc/geoid.html) and [GeoidEval service](https://geographiclib.sourceforge.io/cgi-bin/GeoidEval), C. Karney, accessed 2026-11-06.
- Ahlers, D., [Assessment of the accuracy of GeoNames gazetteer data](https://www.informatik.uni-oldenburg.de/~there/ahlers2013girgeonames.pdf), GIR '13, 2013, doi 10.1145/2533888.2533938.
- [Wikibase data model](https://www.mediawiki.org/wiki/Wikibase/DataModel), Wikimedia, accessed 2026-11-06.
- [OSM wiki: Key:place](https://wiki.openstreetmap.org/wiki/Key:place), accessed 2026-11-06.
- Weiszfeld, E., Sur le point pour lequel la somme des distances de n points donnés est minimum, *Tôhoku Math. J.* 43:355–386, 1937.
- Vardi, Y. & Zhang, C.-H., [The multivariate L1-median and associated data depth](https://www.pnas.org/doi/10.1073/pnas.97.4.1423), *PNAS* 97(4):1423–1426, 2000.
- Lopuhaä, H. P. & Rousseeuw, P. J., Breakdown points of affine equivariant estimators of multivariate location and covariance matrices, *Annals of Statistics* 19(1):229–248, 1991.
- [Google Open Buildings](https://sites.research.google/gr/open-buildings/), Google Research, v3, accessed 2026-11-06.
- [Overture Maps buildings guide](https://docs.overturemaps.org/guides/buildings/), Overture Maps Foundation, accessed 2026-11-06.
- Shi, W., Wang, B., Liu, E., [Relationship between sampling density and DEM error by bilinear and bicubic interpolation](https://www.taylorfrancis.com/chapters/edit/10.1201/b19160-18/qualitative-quantitative-comparative-analysis-relationship-sampling-density-dem-error-bilinear-bicubic-interpolation-methods-wenzhong-shi-bin-wang-eryong-liu), CRC, accessed 2026-11-06.
