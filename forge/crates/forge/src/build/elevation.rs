//! Place elevations (D-109, D-111): the height of the DEM section at each place's final point
//! (the consensus point of the `core` stage, or the point the build gave it), in whole metres
//! above EGM2008, for the pack's ELEVATION section (D-133).
//!
//! - **Input:** the `dem-section` stage's pack, `data/build/dem-section/<tileset>/dem.tgd`,
//!   read through the same reader an app uses (`teigeodata_format::dem`, bilinear).
//! - **Which places:** every place but timezone areas, whose point is only somewhere inside the
//!   zone. A place outside the section has none.
//! - **Rounding:** half away from zero, to whole metres.
//! - **QA, not input (D-061):** the summary compares each height with what the place's own
//!   source records state (`elevation_m`) and with the DEM height `GeoNames` states (`dem_m`).
//! - **Check:** after the pack is written, every row's ELEVATION height is compared with the
//!   section sampled afresh at the row's stored point, a path that shares no code with the
//!   sampling here.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use arrow_array::{Array, Float32Array, Int32Array, StringArray};
use teigeodata_core::{E6, E7, PlaceKind, Point, hilbert};
use teigeodata_format::dem::{Dem, DemScratch};
use teigeodata_format::elevation::Elevations as Heights;
use teigeodata_format::places::Places;
use teigeodata_format::{Kind, Pack};

use super::Place;
use crate::geom::Polygon;
use crate::record::{column, for_each_batch};

/// The DEM a build samples, and the heights it gave.
pub(crate) struct Elevations {
    pub(crate) tileset: String,
    /// The `dem-section` pack, shared by a split build's parts.
    bytes: std::sync::Arc<Vec<u8>>,
    /// One per place, in the build's place order.
    pub(crate) values: Vec<Option<i16>>,
}

/// The DEM section in a `dem-section` pack.
fn open(bytes: &[u8]) -> Result<Dem<'_>, String> {
    let pack = Pack::open(bytes).map_err(|e| e.to_string())?;
    pack.verify_sections().map_err(|e| e.to_string())?;
    let s = pack.section(Kind::DEM).ok_or("no DEM section")?;
    Dem::open(&s).map_err(|e| e.to_string())
}

/// Metres to the stored whole metres: half away from zero, inside `i16` but its minimum.
pub(crate) fn metres(h: f64) -> Result<i16, String> {
    let m = h.round();
    if m.is_finite() && m > f64::from(i16::MIN) && m <= f64::from(i16::MAX) {
        #[allow(clippy::cast_possible_truncation, reason = "checked to fit i16")]
        Ok(m as i16)
    } else {
        Err(format!("a height of {h} m does not fit a place elevation"))
    }
}

/// The section's height at a place's (lon, lat) point on the 1e-7° grid.
fn height(dem: &Dem<'_>, (lon, lat): (i32, i32), s: &mut DemScratch) -> Option<f64> {
    dem.elevation(Point::<E7>::from_units(lat, lon)?, s)
}

/// Each place's height from `dem`, in place order; `None` for timezone areas and points the
/// section does not cover. Visited in Hilbert order so neighbours share a decoded tile.
pub(crate) fn sample(dem: &Dem<'_>, places: &[Place]) -> Result<Vec<Option<i16>>, String> {
    let mut order: Vec<(u64, usize)> = places
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind != PlaceKind::TimezoneArea)
        .filter_map(|(i, p)| {
            Point::<E7>::from_units(p.point.1, p.point.0).map(|at| (hilbert(at), i))
        })
        .collect();
    order.sort_unstable();
    let mut out = vec![None; places.len()];
    let mut s = Box::new(DemScratch::new());
    for (_, i) in order {
        let (Some(p), Some(slot)) = (places.get(i), out.get_mut(i)) else {
            continue;
        };
        *slot = height(dem, p.point, &mut s).map(metres).transpose()?;
    }
    Ok(out)
}

/// Loads the tileset's section and samples every place; `None` without a tileset.
pub(crate) fn load(
    root: &Path,
    tileset: Option<&str>,
    places: &[Place],
) -> Result<Option<Elevations>, String> {
    let Some(tileset) = tileset else {
        return Ok(None);
    };
    let path = root
        .join("data/build/dem-section")
        .join(tileset)
        .join("dem.tgd");
    let bytes = fs::read(&path)
        .map_err(|e| format!("{}: {e}; run forge dem-section first", path.display()))?;
    let values = sample(&open(&bytes)?, places)?;
    Ok(Some(Elevations {
        tileset: tileset.to_string(),
        bytes: std::sync::Arc::new(bytes),
        values,
    }))
}

/// The PLACES elevation column: each row's height, rows given as indices into the build's
/// places; empty (no column) without elevations.
pub(crate) fn in_rows(
    e: Option<&Elevations>,
    rows: impl Iterator<Item = usize>,
) -> Vec<Option<i16>> {
    e.map_or_else(Vec::new, |e| {
        rows.map(|i| e.values.get(i).copied().flatten()).collect()
    })
}

/// |DEM − stated| by (place kind, field): at the place's point, and at the record's own.
type Diffs = BTreeMap<(String, &'static str), (Vec<f64>, Vec<f64>)>;

/// The `q`-quantile of sorted values (nearest rank).
fn quantile(sorted: &[f64], q: f64) -> f64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a slice"
    )]
    let i = ((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1);
    sorted.get(i).copied().unwrap_or(f64::NAN)
}

/// What a source record states about its height (`elevation_m`, `dem_m`), and its own
/// (lon, lat) point.
type Stated = (Option<f32>, Option<f32>, Option<(i32, i32)>);

fn stated(build: &Path, sources: &[String]) -> Result<BTreeMap<String, Stated>, String> {
    let mut out = BTreeMap::new();
    for s in sources {
        for_each_batch(&build.join("read").join(s).join("records.parquet"), |b| {
            let ids = column::<StringArray>(b, "id")?;
            let ele = column::<Float32Array>(b, "elevation_m")?;
            let dem = column::<Float32Array>(b, "dem_m")?;
            let lat = column::<Int32Array>(b, "lat_e7")?;
            let lon = column::<Int32Array>(b, "lon_e7")?;
            let opt = |a: &Float32Array, i: usize| a.is_valid(i).then(|| a.value(i));
            for i in 0..b.num_rows() {
                let at = (lat.is_valid(i) && lon.is_valid(i)).then(|| (lon.value(i), lat.value(i)));
                let v = (opt(ele, i), opt(dem, i), at);
                if v.0.is_some() || v.1.is_some() {
                    out.insert(ids.value(i).to_string(), v);
                }
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// How far a pack's DEM reaches past its outline: 0.05° (about 5 km), so places on the border
/// and just across it keep terrain around them (D-192).
pub(crate) const DEM_HALO_DEG: f64 = 0.05;

impl Elevations {
    /// The DEM section for a pack whose area is `outline` (its units' polygons): the tiles
    /// within [`DEM_HALO_DEG`] of it, copied from the build's DEM (`dem::mask`, no re-coding),
    /// with the section's tile count; `None` without an outline or with nothing near it.
    pub(crate) fn masked(&self, outline: &[&Polygon]) -> Result<Option<(Vec<u8>, u64)>, String> {
        if outline.is_empty() {
            return Ok(None);
        }
        let dem = open(&self.bytes)?;
        let mut keep: HashSet<(u32, u32)> = HashSet::new();
        for g in 0..dem.grids() {
            if let Some(grid) = dem.grid(g) {
                keep.extend(tiles_near(outline, &grid, DEM_HALO_DEG));
            }
        }
        teigeodata_format::dem::mask(&dem, |_, r, c| keep.contains(&(r, c)))
            .map_err(|e| e.to_string())
    }

    /// The heights of the places `at` (indexes into the build's places), in that order: a split
    /// build's part (D-191). The DEM is shared, not copied.
    pub(crate) fn subset(&self, at: &[usize]) -> Self {
        Self {
            tileset: self.tileset.clone(),
            bytes: std::sync::Arc::clone(&self.bytes),
            values: at
                .iter()
                .map(|&i| self.values.get(i).copied().flatten())
                .collect(),
        }
    }

    /// The summary: how many places have a height, their range, and the QA comparison with
    /// what the source records state, by place kind: at the place's point (what the pack
    /// holds) and at the record's own point (the DEMs' disagreement alone, without the move
    /// to the consensus point).
    pub(crate) fn summary(
        &self,
        build: &Path,
        sources: &[String],
        places: &[Place],
    ) -> Result<String, String> {
        let mut h: Vec<i16> = self.values.iter().flatten().copied().collect();
        h.sort_unstable();
        let mut out = format!(
            "Elevation (D-111): {} of {} places from {} at their final point",
            h.len(),
            places.len(),
            self.tileset
        );
        if let (Some(lo), Some(hi)) = (h.first(), h.last()) {
            let mid = h.get(h.len() / 2).copied().unwrap_or_default();
            let _ = write!(out, "; min {lo} m, median {mid} m, max {hi} m");
        }
        out.push_str(".\n");
        let none: Vec<&str> = places
            .iter()
            .zip(&self.values)
            .filter(|(p, v)| v.is_none() && p.kind != PlaceKind::TimezoneArea)
            .map(|(p, _)| p.key.as_str())
            .collect();
        if !none.is_empty() {
            let _ = writeln!(
                out,
                "No height (outside the section): {} places{}.",
                none.len(),
                if none.len() <= 10 {
                    format!(", {}", none.join(", "))
                } else {
                    String::new()
                }
            );
        }
        // |DEM − stated| for each source record of a place, by (kind, field): at the place's
        // point, and at the record's.
        let stated = stated(build, sources)?;
        let dem = open(&self.bytes)?;
        let mut s = Box::new(DemScratch::new());
        let mut diffs = Diffs::new();
        for (p, v) in places.iter().zip(&self.values) {
            let Some(e) = v else { continue };
            for k in &p.sources {
                let Some(&(ele, dem_m, at)) = stated.get(k) else {
                    continue;
                };
                let own = at.and_then(|a| height(&dem, a, &mut s));
                for (field, x) in [("elevation_m", ele), ("dem_m", dem_m)] {
                    let Some(x) = x.map(f64::from) else { continue };
                    for kind in [format!("{:?}", p.kind), "all".into()] {
                        let slot = diffs.entry((kind, field)).or_default();
                        slot.0.push((f64::from(*e) - x).abs());
                        if let Some(o) = own {
                            slot.1.push((o - x).abs());
                        }
                    }
                }
            }
        }
        if diffs.is_empty() {
            return Ok(out);
        }
        out.push_str(
            "\nQA against the heights the source records state (D-061), |DEM − stated| in metres, at the place's point and at the record's own point:\n\n| Kind | Field | Records | P50 | P90 | > 100 m | P50 own | P90 own |\n|---|---|---:|---:|---:|---:|---:|---:|\n",
        );
        for ((kind, field), (mut d, mut o)) in diffs {
            d.sort_by(f64::total_cmp);
            o.sort_by(f64::total_cmp);
            let far = d.iter().filter(|&&x| x > 100.0).count();
            let _ = writeln!(
                out,
                "| {kind} | {field} | {} | {:.0} | {:.0} | {far} | {:.0} | {:.0} |",
                d.len(),
                quantile(&d, 0.5),
                quantile(&d, 0.9),
                quantile(&o, 0.5),
                quantile(&o, 0.9),
            );
        }
        Ok(out)
    }

    /// The read-back check: the ELEVATION section (`heights`, absent when no place has one)
    /// passes its check and has one row per place, and every row's height is the DEM's height
    /// at the row's stored point (1e-6°, a few centimetres from the sampled one), within a
    /// metre; a row with none is a timezone area or outside the DEM.
    pub(crate) fn verify(
        &self,
        places: &Places<'_>,
        heights: Option<&Heights<'_>>,
    ) -> Result<String, String> {
        verify(&open(&self.bytes)?, places, heights)
    }
}

/// See [`Elevations::verify`].
pub(crate) fn verify(
    dem: &Dem<'_>,
    places: &Places<'_>,
    heights: Option<&Heights<'_>>,
) -> Result<String, String> {
    if let Some(h) = heights {
        h.check().map_err(|e| e.to_string())?;
        if h.len() != places.len() {
            return Err(format!(
                "ELEVATION has {} rows, PLACES {}",
                h.len(),
                places.len()
            ));
        }
    }
    let mut s = Box::new(DemScratch::new());
    let (mut with, mut worst) = (0_usize, 0.0_f64);
    for row in 0..places.len() {
        let at: Point<E6> = places.point(row).ok_or("a row without a point")?;
        let stored = heights.and_then(|h| h.get(row));
        let want = if places.kind(row) == Some(PlaceKind::TimezoneArea) {
            None
        } else {
            dem.elevation(at.to_e7(), &mut s)
        };
        match (stored, want) {
            (None, None) => {}
            (Some(m), Some(h)) if (f64::from(m) - h).abs() <= 1.0 => {
                with += 1;
                worst = worst.max((f64::from(m) - h).abs());
            }
            _ => {
                return Err(format!(
                    "row {row} at {at}: ELEVATION {stored:?} m, the DEM {want:?} m"
                ));
            }
        }
    }
    Ok(format!(
        "Elevations: all {} rows agree with the DEM sampled afresh at their stored point ({with} with a height, at most {worst:.2} m apart).",
        places.len()
    ))
}

/// The tiles of `grid` (global tile row, column) within `halo` degrees of the polygons: every
/// tile an edge passes through or whose centre lies inside (even-odd per row of tile centres),
/// grown by the halo in whole tiles. Degrees, not metres: a longitude tile is narrower than a
/// latitude one away from the equator, so the halo is never less than asked.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "tile indices from degrees inside the grid, far below 2^31"
)]
pub(crate) fn tiles_near(
    polys: &[&Polygon],
    grid: &teigeodata_format::dem::Grid,
    halo: f64,
) -> HashSet<(u32, u32)> {
    let side = grid.side() as f64;
    let (t_lat, t_lon) = (
        side * f64::from(grid.lat_step) / 36_000.0,
        side * f64::from(grid.lon_step) / 36_000.0,
    );
    let (r0, c0) = (i64::from(grid.tile_row0), i64::from(grid.tile_col0));
    let (r1, c1) = (
        r0 + i64::from(grid.tile_rows),
        c0 + i64::from(grid.tile_cols),
    );
    let row_of = |lat: f64| ((90.0 - lat) / t_lat).floor() as i64;
    let col_of = |lon: f64| ((lon + 180.0) / t_lon).floor() as i64;
    let deg = |(x, y): (i32, i32)| (f64::from(x) * 1e-7, f64::from(y) * 1e-7);
    let mut hit: HashSet<(i64, i64)> = HashSet::new();
    let edges = || {
        polys.iter().flat_map(|p| &p.rings).flat_map(|ring| {
            let close = ring.last().zip(ring.first()).map(|(a, b)| (*a, *b));
            ring.windows(2)
                .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
                .chain(close)
        })
    };
    // Edges: sampled at under half a tile, so no tile they cross is skipped.
    let step = t_lat.min(t_lon) / 2.0;
    for (a, b) in edges() {
        let ((x0, y0), (x1, y1)) = (deg(a), deg(b));
        let n = ((x1 - x0).hypot(y1 - y0) / step).ceil().max(1.0) as usize;
        for i in 0..=n {
            let t = i as f64 / n as f64;
            hit.insert((row_of(y0 + (y1 - y0) * t), col_of(x0 + (x1 - x0) * t)));
        }
    }
    // Interiors: each row of tile centres against every edge, even-odd.
    let mut xs: Vec<f64> = Vec::new();
    for r in r0..r1 {
        let centre = 90.0 - (r as f64 + 0.5) * t_lat;
        xs.clear();
        for (a, b) in edges() {
            let ((x0, y0), (x1, y1)) = (deg(a), deg(b));
            if (y0 > centre) != (y1 > centre) {
                xs.push(x0 + (centre - y0) / (y1 - y0) * (x1 - x0));
            }
        }
        xs.sort_by(f64::total_cmp);
        for &[w, e] in xs.as_chunks::<2>().0 {
            // Tiles whose centre is in [w, e].
            let west = ((w + 180.0) / t_lon - 0.5).ceil() as i64;
            let east = ((e + 180.0) / t_lon - 0.5).floor() as i64;
            for c in west.max(c0)..=east.min(c1 - 1) {
                hit.insert((r, c));
            }
        }
    }
    let (kr, kc) = ((halo / t_lat).ceil() as i64, (halo / t_lon).ceil() as i64);
    let mut out = HashSet::new();
    for &(r, c) in &hit {
        for rr in (r - kr).max(r0)..=(r + kr).min(r1 - 1) {
            for cc in (c - kc).max(c0)..=(c + kc).min(c1 - 1) {
                out.insert((rr as u32, cc as u32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

    use super::{metres, sample, tiles_near, verify};
    use crate::build::Place;
    use teigeodata_core::{E7, PlaceKind, Point};
    use teigeodata_format::dem::{
        self, DATUM_EGM2008, Dem, Family, Grid, GridInput, THRESHOLDS, TileOptions,
    };
    use teigeodata_format::elevation::{self, Elevations as Heights};
    use teigeodata_format::places::{self, PlaceColumns, Places};
    use teigeodata_format::{Kind, Section};

    /// One 64 × 64 tile at 1″ whose north-west sample is 28.0089° N … 84° E: the plane
    /// `100 + 2·row + 5·column` metres, so a swapped or shifted point is caught.
    const TR: u64 = 3487;
    const TC: u64 = 14850;

    fn section() -> Vec<u8> {
        let q: Vec<i32> = (0..64 * 64)
            .map(|i| 100 + 2 * (i / 64) + 5 * (i % 64))
            .collect();
        let family = Family::standard();
        let code =
            dem::encode_tile(&q, 6, &family, &THRESHOLDS, &TileOptions::default()).expect("tile");
        let grid = Grid {
            tile_row0: u32::try_from(TR).unwrap(),
            tile_col0: u32::try_from(TC).unwrap(),
            tile_rows: 1,
            tile_cols: 1,
            lat_step: 10,
            lon_step: 10,
            step_mm: 1000,
            datum: DATUM_EGM2008,
            tile_log2: 6,
            first_tile: 0,
        };
        let tiles = [Some(code.as_slice())];
        dem::encode(
            &[GridInput {
                grid,
                tiles: &tiles,
            }],
            &family,
            &THRESHOLDS,
        )
        .expect("section")
    }

    /// (lon, lat) on the 1e-7° grid of the sample position (row, column) inside the tile.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "degrees"
    )]
    fn at(row: f64, col: f64) -> (i32, i32) {
        let lat = 90.0 - ((TR * 64) as f64 + row) / 3600.0;
        let lon = ((TC * 64) as f64 + col) / 3600.0 - 180.0;
        ((lon * 1e7).round() as i32, (lat * 1e7).round() as i32)
    }

    fn place(kind: PlaceKind, point: (i32, i32)) -> Place {
        Place {
            key: format!("{point:?}"),
            kind,
            admin_level: 0,
            point,
            names: Vec::new(),
            population: None,
            rank: 0,
            parent: None,
            flags: 0,
            sources: Vec::new(),
        }
    }

    #[test]
    fn places_take_the_height_at_their_own_point() {
        let body = section();
        let dem = Dem::open(&Section::unpacked(Kind::DEM, 1, &body)).expect("open");
        let places = [
            // 100 + 2·10.5 + 5·20.25 = 222.25.
            place(PlaceKind::Settlement, at(10.5, 20.25)),
            // 100 + 2·40 + 5·3.38 = 196.9: rounded, not truncated.
            place(PlaceKind::Locality, at(40.0, 3.38)),
            // 100 + 2·0 + 5·62 = 410, at a sample.
            place(PlaceKind::Admin, at(0.0, 62.0)),
            // A timezone area has none, even inside the section.
            place(PlaceKind::TimezoneArea, at(5.0, 5.0)),
            // Outside the section.
            place(PlaceKind::Settlement, at(-200.0, 5.0)),
        ];
        let got = sample(&dem, &places).expect("sample");
        assert_eq!(got, [Some(222), Some(197), Some(410), None, None]);
    }

    #[test]
    fn metres_round_half_away_from_zero_and_refuse_what_does_not_fit() {
        assert_eq!(metres(195.5), Ok(196));
        assert_eq!(metres(-0.5), Ok(-1));
        assert_eq!(metres(8_848.86), Ok(8_849));
        assert!(metres(-32_768.0).is_err());
        assert!(metres(40_000.0).is_err());
        assert!(metres(f64::NAN).is_err());
    }

    #[test]
    fn the_read_back_check_catches_a_wrong_height() {
        let body = section();
        let dem = Dem::open(&Section::unpacked(Kind::DEM, 1, &body)).expect("open");
        let p = at(10.5, 20.25);
        let point = Point::<E7>::from_units(p.1, p.0).unwrap().to_e6();
        let check = |e: Option<i16>, kind: PlaceKind| {
            let cols = PlaceColumns {
                points: &[point],
                kinds: &[kind],
                ..PlaceColumns::default()
            };
            let bytes = places::encode(&cols).expect("encode");
            let section = Section::unpacked(Kind::PLACES, 1, &bytes);
            let h = elevation::encode(&[e]).expect("encode");
            let hs = h.as_ref().map(|h| Section::unpacked(Kind::ELEVATION, 1, h));
            let heights = hs.as_ref().map(|s| Heights::open(s).expect("open"));
            verify(
                &dem,
                &Places::open(&section).expect("open"),
                heights.as_ref(),
            )
        };
        assert!(check(Some(222), PlaceKind::Settlement).is_ok());
        assert!(
            check(Some(225), PlaceKind::Settlement).is_err(),
            "a wrong height"
        );
        assert!(
            check(None, PlaceKind::Settlement).is_err(),
            "a missing height"
        );
        assert!(
            check(Some(222), PlaceKind::TimezoneArea).is_err(),
            "a zone's height"
        );
        assert!(check(None, PlaceKind::TimezoneArea).is_ok());
        // ELEVATION must have one row per place.
        let cols = PlaceColumns {
            points: &[point, point],
            kinds: &[PlaceKind::Settlement; 2],
            ..PlaceColumns::default()
        };
        let bytes = places::encode(&cols).expect("encode");
        let two = Places::open(&Section::unpacked(Kind::PLACES, 2, &bytes)).expect("open");
        let h = elevation::encode(&[Some(222)])
            .expect("encode")
            .expect("known");
        let one = Heights::open(&Section::unpacked(Kind::ELEVATION, 1, &h)).expect("open");
        assert!(verify(&dem, &two, Some(&one)).is_err(), "a short ELEVATION");
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "degrees to tile indices in the test grid"
    )]
    fn tiles_near_an_outline_are_its_inside_and_its_halo() {
        let grid = Grid {
            tile_row0: 3_400,
            tile_col0: 14_800,
            tile_rows: 300,
            tile_cols: 300,
            lat_step: 10,
            lon_step: 10,
            step_mm: 2_000,
            datum: DATUM_EGM2008,
            tile_log2: 6,
            first_tile: 0,
        };
        let e7 = |d: f64| (d * 1e7).round() as i32;
        let square = |w: f64, s: f64, e: f64, n: f64| crate::geom::Polygon {
            rings: vec![vec![
                (e7(w), e7(s)),
                (e7(e), e7(s)),
                (e7(e), e7(n)),
                (e7(w), e7(n)),
            ]],
        };
        // A 0.6° square with a 0.2° hole in its middle (the hole is still within no halo).
        let mut p = square(84.2, 26.0, 84.8, 26.6);
        p.rings.push(square(84.4, 26.2, 84.6, 26.4).rings.remove(0));
        let kept = tiles_near(&[&p], &grid, 0.05);
        let tile = |lon: f64, lat: f64| {
            let t = 64.0 / 3600.0;
            (((90.0 - lat) / t) as u32, ((lon + 180.0) / t) as u32)
        };
        assert!(kept.contains(&tile(84.3, 26.1)), "inside");
        assert!(
            kept.contains(&tile(84.84, 26.3)),
            "0.04° outside, in the halo"
        );
        assert!(!kept.contains(&tile(84.9, 26.3)), "0.1° outside");
        assert!(!kept.contains(&tile(85.5, 26.3)), "far outside");
        assert!(
            kept.contains(&tile(84.5, 26.23)),
            "in the hole, within the halo of its edge"
        );
        assert!(
            !kept.contains(&tile(84.5, 26.3)),
            "the hole's middle, 0.1° from its edges"
        );
        // Every kept tile is in the grid.
        assert!(
            kept.iter()
                .all(|&(r, c)| (3_400..3_700).contains(&r) && (14_800..15_100).contains(&c))
        );
    }
}
