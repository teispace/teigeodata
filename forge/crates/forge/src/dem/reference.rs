//! `forge dem-ref <dem-tileset> <reference-tileset> <geoid-source>`: the DEM against an
//! independent ground reference (spike S6, elevation-reference note). The reference is ICESat-2
//! ATL08 terrain (`h_te_best_fit`, WGS84 ellipsoidal) from `SlideRule` responses; the geoid turns
//! it into EGM2008 orthometric height, the DEM's datum: H = h − N.
//!
//! Reference tiles are processed [`PARALLEL`] at a time, each with its one DEM tile. Statistics
//! go into fixed 0.1 m histograms per stratum, so memory does not grow with the number of
//! points. Every tile's points are written to `data/build/dem-ref/<dem>/<reference>/<tile>.parquet`
//! for calibration.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use arrow_array::{Array, Float32Array, Float64Array, RecordBatch, UInt8Array};
use arrow_schema::{DataType, Field};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rayon::prelude::*;

use super::{Grid, PARALLEL, SLOPE_BANDS};
use crate::geoid::Geoid;
use crate::lock::Lock;
use crate::record::{bools, column, e7, f32s, i32s, write_table};
use crate::sliderule;
use crate::tiles::{self, Tile};

/// Histogram range and resolution: DEM − reference in [−`SPAN`, +`SPAN`] m, 0.1 m bins.
const SPAN: f64 = 2000.0;
const BINS_PER_M: f64 = 10.0;
/// Reference-height bands (orthometric metres, upper bounds).
const HEIGHT_BANDS: [f32; 5] = [500.0, 1500.0, 3000.0, 4500.0, f32::INFINITY];
/// Land cover groups of ATL08's `segment_landcover` (Copernicus LC100 classes).
const COVER: [&str; 8] = [
    "forest",
    "cropland",
    "shrub and grass",
    "built-up",
    "bare",
    "snow and ice",
    "water and wetland",
    "other",
];

fn cover_group(class: u8) -> usize {
    match class {
        111..=126 => 0,
        40 => 1,
        20 | 30 | 100 => 2,
        50 => 3,
        60 => 4,
        70 => 5,
        80 | 90 | 200 => 6,
        _ => 7,
    }
}

/// Stratum layout: all, slope bands, cover groups, snow (no, yes), light (day, night), height.
const STRATA: usize = 1 + SLOPE_BANDS.len() + COVER.len() + 2 + 2 + HEIGHT_BANDS.len();

/// A signed histogram with exact sums for mean and RMSE.
#[derive(Clone)]
struct Hist {
    bins: Vec<u32>,
    /// Values below −`SPAN` (those above are `n` minus the rest).
    under: u64,
    n: u64,
    sum: f64,
    sum_sq: f64,
}

impl Hist {
    fn new() -> Self {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a constant size"
        )]
        let len = (2.0 * SPAN * BINS_PER_M) as usize + 1;
        Self {
            bins: vec![0; len],
            under: 0,
            n: 0,
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    fn add(&mut self, d: f64) {
        self.n += 1;
        self.sum += d;
        self.sum_sq += d * d;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked to be in range"
        )]
        let k = ((d + SPAN) * BINS_PER_M).round() as usize;
        if d.abs() <= SPAN
            && let Some(b) = self.bins.get_mut(k)
        {
            *b += 1;
        } else if d < 0.0 {
            self.under += 1;
        }
    }

    fn merge(&mut self, o: &Self) {
        for (a, b) in self.bins.iter_mut().zip(&o.bins) {
            *a += b;
        }
        (self.under, self.n, self.sum, self.sum_sq) = (
            self.under + o.under,
            self.n + o.n,
            self.sum + o.sum,
            self.sum_sq + o.sum_sq,
        );
    }

    /// Bin centre of the signed q-quantile (values beyond the span count at its ends).
    fn signed(&self, q: f64) -> f64 {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "counts"
        )]
        let target = ((q * self.n as f64).ceil() as u64).max(1);
        let mut seen = self.under;
        if seen >= target {
            return -SPAN;
        }
        for (k, &c) in self.bins.iter().enumerate() {
            seen += u64::from(c);
            if seen >= target {
                #[allow(clippy::cast_precision_loss, reason = "a bin index")]
                return k as f64 / BINS_PER_M - SPAN;
            }
        }
        SPAN
    }

    /// The q-quantile of |d|, folding the histogram about zero.
    fn absolute(&self, q: f64) -> f64 {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "counts"
        )]
        let target = ((q * self.n as f64).ceil() as u64).max(1);
        let mid = self.bins.len() / 2;
        let mut seen = u64::from(self.bins.get(mid).copied().unwrap_or(0));
        for r in 1..=mid {
            if seen >= target {
                #[allow(clippy::cast_precision_loss, reason = "a bin index")]
                return (r - 1) as f64 / BINS_PER_M;
            }
            seen += u64::from(self.bins.get(mid - r).copied().unwrap_or(0))
                + u64::from(self.bins.get(mid + r).copied().unwrap_or(0));
        }
        if seen >= target { SPAN } else { f64::INFINITY }
    }

    /// Share of |d| above `limit` metres.
    fn share_over(&self, limit: f64) -> f64 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a small constant"
        )]
        let r = (limit * BINS_PER_M).round() as usize;
        let mid = self.bins.len() / 2;
        let inside: u64 = self
            .bins
            .get(mid - r..=mid + r)
            .map_or(0, |s| s.iter().map(|&c| u64::from(c)).sum());
        #[allow(clippy::cast_precision_loss, reason = "counts")]
        let share = 100.0 * (self.n - inside) as f64 / self.n.max(1) as f64;
        share
    }
}

/// One reference point with the DEM sampled at it.
struct Point {
    lat: f64,
    lon: f64,
    reference: f32,
    dem: f32,
    slope: f32,
    cover: u8,
    snow: bool,
    night: bool,
}

/// What one tile contributes.
struct Part {
    hists: Vec<Hist>,
    read: usize,
    dropped: usize,
    empty_beams: usize,
}

pub(crate) fn run(
    root: &Path,
    dem: &str,
    reference: &str,
    geoid: &str,
    within: Option<&str>,
) -> Result<String, String> {
    let t = Instant::now();
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let dem_src = lock.get(dem)?;
    let template = dem_src
        .query_params
        .get("tile")
        .ok_or("the DEM tileset has no tile template")?;
    let dem_tiles: std::collections::BTreeMap<String, Tile> = tiles::manifest(root, dem_src)?
        .into_iter()
        .map(|t| (t.name.clone(), t))
        .collect();
    let refs: Vec<Tile> = tiles::manifest(root, lock.get(reference)?)?
        .into_iter()
        .filter(|t| t.sha256.is_some())
        .collect();
    let open = |id: &str| {
        lock.get(id)
            .and_then(|g| Geoid::open_zip(&root.join("data/cache/store/sha256").join(&g.sha256)))
    };
    // A DEM on another geoid (`query_params.geoid`, e.g. EGM96 for SRTM and NASADEM) is moved
    // to the reference geoid: H = H_dem + N_dem − N_ref.
    let dem_geoid = match dem_src.query_params.get("geoid") {
        Some(g) if g != geoid => Some(open(g)?),
        _ => None,
    };
    let geoid = open(geoid)?;
    let region = within
        .map(|b| crate::pip::Region::load(root, b))
        .transpose()?;
    let run = within.map_or_else(|| reference.to_string(), |b| format!("{reference}+{b}"));
    let dir = root.join("data/build/dem-ref").join(dem).join(&run);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(PARALLEL)
        .build()
        .map_err(|e| e.to_string())?;
    let ctx = Ctx {
        root,
        template,
        dem_tiles: &dem_tiles,
        geoid: &geoid,
        dem_geoid: dem_geoid.as_ref(),
        dir: &dir,
        region: region.as_ref(),
    };
    let parts: Vec<Part> = pool.install(|| {
        refs.par_iter()
            .map(|r| ctx.tile(r))
            .collect::<Result<_, String>>()
    })?;
    let mut hists = vec![Hist::new(); STRATA];
    let mut totals = Totals {
        tiles: refs.len(),
        ..Totals::default()
    };
    for p in &parts {
        for (a, b) in hists.iter_mut().zip(&p.hists) {
            a.merge(b);
        }
        totals.read += p.read;
        totals.dropped += p.dropped;
        totals.empty_beams += p.empty_beams;
    }
    let summary = report(dem, &run, &hists, &totals);
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    Ok(format!("{summary}\n{:.1?}", t.elapsed()))
}

struct Ctx<'a> {
    root: &'a Path,
    template: &'a str,
    dem_tiles: &'a std::collections::BTreeMap<String, Tile>,
    geoid: &'a Geoid,
    dem_geoid: Option<&'a Geoid>,
    dir: &'a Path,
    region: Option<&'a crate::pip::Region>,
}

impl Ctx<'_> {
    fn tile(&self, r: &Tile) -> Result<Part, String> {
        let path = tiles::tile_path(self.root, r).ok_or("an absent tile")?;
        let stream = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Empty beams were confirmed by `forge fetch` before the bytes were pinned.
        let response = sliderule::read(&stream).map_err(|e| format!("{}: {e}", r.name))?;
        let empty_beams = response.empty_beams.len();
        let file = response.file;
        let reader = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(file))
            .map_err(|e| format!("{}: {e}", r.name))?
            .build()
            .map_err(|e| format!("{}: {e}", r.name))?;
        let mut raw = Vec::new();
        for b in reader {
            read_batch(&b.map_err(|e| e.to_string())?, &mut raw)?;
        }
        let read = raw.len();
        let mut hists = vec![Hist::new(); STRATA];
        let mut points = Vec::with_capacity(raw.len());
        // One DEM tile per reference tile: both are 1° cells on the same corners.
        let mut grid: Option<(String, Option<Grid>)> = None;
        let mut outside = 0;
        for (lat, lon, h, cover, snow, night) in raw {
            if let Some(reg) = self.region
                && !matches!((e7(lon), e7(lat)), (Some(x), Some(y)) if reg.contains(x, y))
            {
                outside += 1;
                continue;
            }
            #[allow(clippy::cast_possible_truncation, reason = "whole degrees")]
            let name = tiles::tile_name(self.template, lon.floor() as i32, lat.floor() as i32);
            if grid.as_ref().is_none_or(|(n, _)| *n != name) {
                let g = match self
                    .dem_tiles
                    .get(&name)
                    .and_then(|t| tiles::tile_path(self.root, t))
                {
                    Some(p) => Some(Grid::open(&p)?),
                    None => None,
                };
                grid = Some((name, g));
            }
            let Some((dem, slope)) = grid
                .as_ref()
                .and_then(|(_, g)| g.as_ref())
                .and_then(|g| g.sample(lon, lat))
            else {
                continue;
            };
            let n_ref = self.geoid.undulation(lat, lon);
            #[allow(clippy::cast_possible_truncation, reason = "heights in metres")]
            let (reference, dem) = (
                (f64::from(h) - n_ref) as f32,
                self.dem_geoid.map_or(dem, |g| {
                    (f64::from(dem) + g.undulation(lat, lon) - n_ref) as f32
                }),
            );
            let p = Point {
                lat,
                lon,
                reference,
                dem,
                slope,
                cover,
                snow,
                night,
            };
            for s in strata(&p) {
                if let Some(hist) = hists.get_mut(s) {
                    hist.add(f64::from(p.dem) - f64::from(p.reference));
                }
            }
            points.push(p);
        }
        let read = read - outside;
        let dropped = read - points.len();
        write_points(&self.dir.join(format!("{}.parquet", r.name)), &points)?;
        Ok(Part {
            hists,
            read,
            dropped,
            empty_beams,
        })
    }
}

/// (lat, lon, `h_te_best_fit`, land cover, snow, night) of every valid segment in a batch.
type Raw = (f64, f64, f32, u8, bool, bool);

fn read_batch(b: &RecordBatch, out: &mut Vec<Raw>) -> Result<(), String> {
    let (lat, lon) = (
        column::<Float64Array>(b, "latitude")?,
        column::<Float64Array>(b, "longitude")?,
    );
    let h = column::<Float32Array>(b, "terrain/h_te_best_fit")?;
    let cover = column::<UInt8Array>(b, "segment_landcover")?;
    let snow = column::<UInt8Array>(b, "segment_snowcover")?;
    let sun = column::<Float32Array>(b, "solar_elevation")?;
    for i in 0..b.num_rows() {
        let v = h.value(i);
        // Fill values (3.4e38, NaN) and the impossible are not measurements.
        if h.is_valid(i)
            && v.is_finite()
            && v.abs() < 10_000.0
            && lat.is_valid(i)
            && lon.is_valid(i)
        {
            out.push((
                lat.value(i),
                lon.value(i),
                v,
                cover.value(i),
                matches!(snow.value(i), 2 | 3),
                sun.value(i) < 0.0,
            ));
        }
    }
    Ok(())
}

/// The strata a point belongs to, as indexes into the histogram list.
fn strata(p: &Point) -> [usize; 6] {
    let slope = SLOPE_BANDS
        .iter()
        .position(|&hi| p.slope < hi)
        .unwrap_or(SLOPE_BANDS.len() - 1);
    let height = HEIGHT_BANDS
        .iter()
        .position(|&hi| p.reference < hi)
        .unwrap_or(HEIGHT_BANDS.len() - 1);
    let base_cover = 1 + SLOPE_BANDS.len();
    let base_snow = base_cover + COVER.len();
    [
        0,
        1 + slope,
        base_cover + cover_group(p.cover),
        base_snow + usize::from(p.snow),
        base_snow + 2 + usize::from(p.night),
        base_snow + 4 + height,
    ]
}

fn labels() -> Vec<String> {
    let mut v = vec!["all".to_string()];
    let mut lo = 0.0;
    for hi in SLOPE_BANDS {
        v.push(format!("slope {lo:.0}–{hi:.0}°"));
        lo = hi;
    }
    v.extend(COVER.iter().map(|c| (*c).to_string()));
    v.extend(["no snow", "snow", "day", "night"].map(str::to_string));
    let mut lo = 0.0;
    for hi in HEIGHT_BANDS {
        v.push(if hi.is_finite() {
            format!("height {lo:.0}–{hi:.0} m")
        } else {
            format!("height ≥ {lo:.0} m")
        });
        lo = hi;
    }
    v
}

/// Counts over all reference tiles.
#[derive(Default)]
struct Totals {
    tiles: usize,
    read: usize,
    dropped: usize,
    empty_beams: usize,
}

fn report(dem: &str, reference: &str, hists: &[Hist], t: &Totals) -> String {
    let mut out = format!(
        "## dem-ref: {dem} against {reference}\n\n{} reference tiles ({} beams with no land segments, confirmed at fetch), {} valid segments, {} outside the DEM or on a void. Δ = DEM − reference (EGM2008 orthometric), metres; bias is the median Δ; LE90 is the P90 of |Δ|.\n\n| Stratum | Points | Bias | Mean | RMSE | P50 \\|Δ\\| | LE90 | P99 \\|Δ\\| | > 50 m |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        t.tiles, t.empty_beams, t.read, t.dropped
    );
    for (label, h) in labels().iter().zip(hists) {
        if h.n == 0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss, reason = "counts")]
        let n = h.n as f64;
        let _ = writeln!(
            out,
            "| {label} | {} | {:+.1} | {:+.2} | {:.1} | {:.1} | {:.1} | {:.1} | {:.2}% |",
            h.n,
            h.signed(0.5),
            h.sum / n,
            (h.sum_sq / n).sqrt(),
            h.absolute(0.5),
            h.absolute(0.9),
            h.absolute(0.99),
            h.share_over(50.0)
        );
    }
    out
}

fn write_points(path: &Path, points: &[Point]) -> Result<(), String> {
    let f = |n: &str| Field::new(n, DataType::Float32, false);
    write_table(
        path,
        vec![
            Field::new("lat_e7", DataType::Int32, false),
            Field::new("lon_e7", DataType::Int32, false),
            f("reference_m"),
            f("dem_m"),
            f("slope_deg"),
            Field::new("landcover", DataType::Int32, false),
            Field::new("snow", DataType::Boolean, false),
            Field::new("night", DataType::Boolean, false),
        ],
        points,
        |r| {
            vec![
                i32s(r.iter().map(|p| e7(p.lat))),
                i32s(r.iter().map(|p| e7(p.lon))),
                f32s(r.iter().map(|p| Some(p.reference))),
                f32s(r.iter().map(|p| Some(p.dem))),
                f32s(r.iter().map(|p| Some(p.slope))),
                i32s(r.iter().map(|p| Some(i32::from(p.cover)))),
                bools(r.iter().map(|p| p.snow)),
                bools(r.iter().map(|p| p.night)),
            ]
        },
    )
}

#[cfg(test)]
mod tests {
    use super::Hist;

    #[test]
    fn histogram_quantiles() {
        let mut h = Hist::new();
        for d in [-3.0, -1.0, 0.0, 2.0, 10.0, 60.0, -2500.0] {
            h.add(d);
        }
        assert_eq!(h.n, 7);
        assert!((h.signed(0.5) - 0.0).abs() < 1e-9, "{}", h.signed(0.5));
        // |d| sorted: 0, 1, 2, 3, 10, 60, 2500 → P50 is 3.
        assert!((h.absolute(0.5) - 3.0).abs() < 1e-9, "{}", h.absolute(0.5));
        assert!(h.absolute(1.0).is_infinite(), "beyond the span");
        assert!((h.share_over(50.0) - 200.0 / 7.0).abs() < 1e-9);
        let mut m = Hist::new();
        m.merge(&h);
        assert_eq!(m.n, 7);
    }
}
