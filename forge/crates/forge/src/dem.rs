//! The `dem` stage (forge note §S5, metric 4): samples a DEM tileset at every record's point
//! and compares it with the elevation each source states. It also samples both points of every
//! stated cross-source link (the `links` output), which measures how much height the position
//! disagreement alone costs: the error a wrong point puts into a DEM-sampled elevation.
//!
//! Tiles are decoded once each, [`PARALLEL`] at a time, so memory stays bounded at any extent.
//! Sampling is bilinear between pixel centres. The grid comes from the `GeoTIFF` tags
//! (`ModelPixelScale`, `ModelTiepoint` and the `GeoKey` raster type, so `PixelIsPoint` tiles
//! such as Copernicus are not shifted by half a pixel). Slope uses Horn's 3×3 method in metres.
//! Heights are compared as stated: geoid differences between sources (EGM96 and EGM2008) are
//! not corrected here.
//!
//! Output: `data/build/dem/<tileset>/<source>.parquet` (`id`, `dem_m`, `slope_deg`,
//! `stated_m`, `source_dem_m`) and `summary.md`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;

use arrow_array::{Array, Float32Array, Int32Array, StringArray};
use arrow_schema::{DataType, Field};
use rayon::prelude::*;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

pub(crate) mod reference;

use crate::lock::Lock;
use crate::record::{self, column, f32s, strings, write_table};
use crate::tiles::{self, Tile};

/// Tiles decoded at once (each Copernicus tile is 52 MB as f32).
const PARALLEL: usize = 4;
/// Metres per degree of latitude (mean Earth radius).
const M_PER_DEG: f64 = 111_195.08;
/// Slope bands for the error table, in degrees.
const SLOPE_BANDS: [f32; 4] = [5.0, 15.0, 30.0, 90.0];

/// A decoded tile: pixel centres at `lon0 + i·dx`, `lat0 − j·dy`.
struct Grid {
    lon0: f64,
    lat0: f64,
    dx: f64,
    dy: f64,
    w: usize,
    h: usize,
    z: Vec<f32>,
}

impl Grid {
    fn open(path: &Path) -> Result<Self, String> {
        let err = |e: tiff::TiffError| format!("{}: {e}", path.display());
        let mut d = Decoder::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?))
            .map_err(err)?;
        let (w, h) = d.dimensions().map_err(err)?;
        let scale = d.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(err)?;
        let tie = d.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(err)?;
        let keys = d
            .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
            .unwrap_or_default();
        let (Some(&dx), Some(&dy), Some(&i0), Some(&j0), Some(&x0), Some(&y0)) = (
            scale.first(),
            scale.get(1),
            tie.first(),
            tie.get(1),
            tie.get(3),
            tie.get(4),
        ) else {
            return Err(format!("{}: missing georeferencing tags", path.display()));
        };
        // GeoKey 1025 (GTRasterTypeGeoKey): 1 = PixelIsArea (tie point at a pixel's corner), 2 =
        // PixelIsPoint (at its centre). Keys are 4-tuples after a 4-short header.
        let is_point = keys
            .chunks(4)
            .skip(1)
            .any(|k| k.first() == Some(&1025) && k.get(3) == Some(&2));
        let half = if is_point { 0.0 } else { 0.5 };
        let DecodingResult::F32(z) = d.read_image().map_err(err)? else {
            return Err(format!("{}: not a Float32 raster", path.display()));
        };
        Ok(Self {
            lon0: x0 + (half - i0) * dx,
            lat0: y0 - (half - j0) * dy,
            dx,
            dy,
            w: usize::try_from(w).unwrap_or(0),
            h: usize::try_from(h).unwrap_or(0),
            z,
        })
    }

    fn at(&self, i: usize, j: usize) -> Option<f32> {
        let (i, j) = (
            i.min(self.w.saturating_sub(1)),
            j.min(self.h.saturating_sub(1)),
        );
        self.z
            .get(j * self.w + i)
            .copied()
            .filter(|v| v.is_finite() && *v > -1000.0)
    }

    /// Bilinear height and Horn slope (degrees) at a point inside the tile.
    fn sample(&self, lon: f64, lat: f64) -> Option<(f32, f32)> {
        let (fx, fy) = ((lon - self.lon0) / self.dx, (self.lat0 - lat) / self.dy);
        if fx < -0.5 || fy < -0.5 {
            return None;
        }
        let (fx, fy) = (fx.max(0.0), fy.max(0.0));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "non-negative, inside the tile"
        )]
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        if i >= self.w || j >= self.h {
            return None;
        }
        #[allow(clippy::cast_precision_loss, reason = "pixel indices")]
        let (tx, ty) = (fx - i as f64, fy - j as f64);
        let z = |a: usize, b: usize| self.at(a, b).map(f64::from);
        let top = z(i, j)? * (1.0 - tx) + z(i + 1, j)? * tx;
        let bottom = z(i, j + 1)? * (1.0 - tx) + z(i + 1, j + 1)? * tx;
        #[allow(clippy::cast_possible_truncation, reason = "a height in metres")]
        let height = (top * (1.0 - ty) + bottom * ty) as f32;
        Some((height, self.slope(i, j, lat)?))
    }

    fn slope(&self, i: usize, j: usize, lat: f64) -> Option<f32> {
        let z = |di: isize, dj: isize| {
            let a = i.checked_add_signed(di).unwrap_or(0);
            let b = j.checked_add_signed(dj).unwrap_or(0);
            self.at(a, b).map(f64::from)
        };
        // Horn's 3×3 window, rows north to south.
        let mut win = [[0.0_f64; 3]; 3];
        for (dj, row) in (-1_isize..=1).zip(win.iter_mut()) {
            for (di, cell) in (-1_isize..=1).zip(row.iter_mut()) {
                *cell = z(di, dj)?;
            }
        }
        let [[nw, north, ne], [west, _, east], [sw, south, se]] = win;
        let mx = self.dx * M_PER_DEG * lat.to_radians().cos();
        let my = self.dy * M_PER_DEG;
        let east_slope = ((ne + 2.0 * east + se) - (nw + 2.0 * west + sw)) / (8.0 * mx);
        let north_slope = ((sw + 2.0 * south + se) - (nw + 2.0 * north + ne)) / (8.0 * my);
        #[allow(clippy::cast_possible_truncation, reason = "degrees")]
        let deg = east_slope.hypot(north_slope).atan().to_degrees() as f32;
        Some(deg)
    }
}

/// A sampled (height, slope), or `None` outside the tileset or on a void.
type Sample = Option<(f32, f32)>;

/// A record with what it says about its height.
struct Rec {
    id: String,
    lon: f64,
    lat: f64,
    stated: Option<f32>,
    source_dem: Option<f32>,
}

fn read_recs(path: &Path) -> Result<Vec<Rec>, String> {
    let mut out = Vec::new();
    record::for_each_batch(path, |b| {
        let ids = column::<StringArray>(b, "id")?;
        let (lat, lon) = (
            column::<Int32Array>(b, "lat_e7")?,
            column::<Int32Array>(b, "lon_e7")?,
        );
        let (ele, dem) = (
            column::<Float32Array>(b, "elevation_m")?,
            column::<Float32Array>(b, "dem_m")?,
        );
        let opt = |a: &Float32Array, i: usize| a.is_valid(i).then(|| a.value(i));
        for i in 0..b.num_rows() {
            if lat.is_valid(i) && lon.is_valid(i) {
                out.push(Rec {
                    id: ids.value(i).to_string(),
                    lon: f64::from(lon.value(i)) * 1e-7,
                    lat: f64::from(lat.value(i)) * 1e-7,
                    stated: opt(ele, i),
                    source_dem: opt(dem, i),
                });
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// Samples the tileset at each point: (height, slope) or `None` outside it.
fn sample_all(
    root: &Path,
    template: &str,
    manifest: &[Tile],
    pts: &[(f64, f64)],
) -> Result<Vec<Sample>, String> {
    let by_name: BTreeMap<&str, &Tile> = manifest.iter().map(|t| (t.name.as_str(), t)).collect();
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (k, (lon, lat)) in pts.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation, reason = "whole degrees")]
        let name = tiles::tile_name(template, lon.floor() as i32, lat.floor() as i32);
        groups.entry(name).or_default().push(k);
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(PARALLEL)
        .build()
        .map_err(|e| e.to_string())?;
    let parts: Vec<Vec<(usize, Sample)>> = pool.install(|| {
        groups
            .par_iter()
            .map(|(name, idx)| {
                let Some(path) = by_name
                    .get(name.as_str())
                    .and_then(|t| tiles::tile_path(root, t))
                else {
                    return Ok(idx.iter().map(|&k| (k, None)).collect());
                };
                let g = Grid::open(&path)?;
                Ok(idx
                    .iter()
                    .map(|&k| (k, pts.get(k).and_then(|&(x, y)| g.sample(x, y))))
                    .collect())
            })
            .collect::<Result<_, String>>()
    })?;
    let mut out = vec![None; pts.len()];
    for (k, v) in parts.into_iter().flatten() {
        if let Some(slot) = out.get_mut(k) {
            *slot = v;
        }
    }
    Ok(out)
}

/// `forge dem <tileset-source> <points-source> [...]`.
pub(crate) fn run(root: &Path, tileset: &str, sources: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let s = lock.get(tileset)?;
    let template = s
        .query_params
        .get("tile")
        .ok_or("the tileset has no tile template")?;
    let manifest = tiles::manifest(root, s)?;
    let dir = root.join("data/build/dem").join(tileset);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut summary = format!(
        "## dem: {tileset}\n\nStated elevation minus the DEM at the record's own point, in metres (bias is the median signed difference).\n\n| Source | Field | Points | Bias | P50 | P90 | P99 | > 50 m |\n|---|---|---:|---:|---:|---:|---:|---:|\n"
    );
    let mut by_slope = String::new();
    let mut heights: BTreeMap<String, (f32, f32)> = BTreeMap::new();
    for name in sources {
        let recs = read_recs(
            &root
                .join("data/build/read")
                .join(name)
                .join("records.parquet"),
        )?;
        let pts: Vec<(f64, f64)> = recs.iter().map(|r| (r.lon, r.lat)).collect();
        let sampled = sample_all(root, template, &manifest, &pts)?;
        for (r, v) in recs.iter().zip(&sampled) {
            if let Some((h, slope)) = v {
                heights.insert(r.id.clone(), (*h, *slope));
            }
        }
        write_source(&dir, name, &recs, &sampled)?;
        for (field, pick) in [("stated", 0), ("source DEM", 1)] {
            let diffs: Vec<(f32, f32)> = recs
                .iter()
                .zip(&sampled)
                .filter_map(|(r, v)| {
                    let stated = if pick == 0 { r.stated } else { r.source_dem }?;
                    v.map(|(h, slope)| (stated - h, slope))
                })
                .collect();
            if diffs.is_empty() {
                continue;
            }
            row(
                &mut summary,
                &format!("| {name} | {field} "),
                diffs.iter().map(|d| d.0).collect(),
            );
            if pick == 0 {
                let mut lo = 0.0;
                for hi in SLOPE_BANDS {
                    let band: Vec<f32> = diffs
                        .iter()
                        .filter(|d| d.1 >= lo && d.1 < hi)
                        .map(|d| d.0)
                        .collect();
                    if !band.is_empty() {
                        row(
                            &mut by_slope,
                            &format!("| {name} | {lo:.0}–{hi:.0}° "),
                            band,
                        );
                    }
                    lo = hi;
                }
            }
        }
    }
    let _ = write!(
        summary,
        "\nThe same by the DEM slope at the point (stated elevation only).\n\n| Source | Slope | Points | Bias | P50 | P90 | P99 | > 50 m |\n|---|---|---:|---:|---:|---:|---:|---:|\n{by_slope}"
    );
    linked_heights(root, sources, &heights, &mut summary)?;
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    Ok(format!("{summary}\n{:.1?}", t.elapsed()))
}

/// The DEM height difference between the two points of every stated link (`links` output),
/// excluding copies: the vertical cost of position disagreement.
fn linked_heights(
    root: &Path,
    sources: &[String],
    heights: &BTreeMap<String, (f32, f32)>,
    out: &mut String,
) -> Result<(), String> {
    let path = root
        .join("data/build/links")
        .join(sources.join("+"))
        .join("pairs.parquet");
    if !path.is_file() {
        return Ok(());
    }
    let mut groups: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    // Per slope band (the mean of both points): metres apart, observed |Δh|, d·tan(slope).
    let mut bands: [Banded; SLOPE_BANDS.len()] = Default::default();
    record::for_each_batch(&path, |b| {
        let (a, bb, m) = (
            column::<StringArray>(b, "a")?,
            column::<StringArray>(b, "b")?,
            column::<Float32Array>(b, "metres")?,
        );
        for i in 0..b.num_rows() {
            let metres = m.value(i);
            if metres < 1.0 {
                continue;
            }
            let (Some(&(ha, sa)), Some(&(hb, sb))) =
                (heights.get(a.value(i)), heights.get(bb.value(i)))
            else {
                continue;
            };
            let scheme = |s: &str| s.split(':').next().unwrap_or("").to_string();
            groups
                .entry(format!("{}–{}", scheme(a.value(i)), scheme(bb.value(i))))
                .or_default()
                .push(ha - hb);
            let slope = f32::midpoint(sa, sb);
            if let Some(band) = SLOPE_BANDS
                .iter()
                .position(|&hi| slope < hi)
                .and_then(|k| bands.get_mut(k))
            {
                band.metres.push(metres);
                band.observed.push((ha - hb).abs());
                band.expected.push(metres * slope.to_radians().tan());
                band.gradient.push(1000.0 * (ha - hb).abs() / metres);
            }
        }
        Ok(())
    })?;
    let _ = write!(
        out,
        "\nDEM height at one source's point minus the DEM height at the other's, for every stated link that is not a copy: the vertical error that position disagreement alone causes.\n\n| Pair | Field | Pairs | Bias | P50 | P90 | P99 | > 50 m |\n|---|---|---:|---:|---:|---:|---:|---:|\n"
    );
    for (pair, v) in groups {
        row(out, &format!("| {pair} | DEM at both points "), v);
    }
    let _ = write!(
        out,
        "\nThe same links by the mean DEM slope of both points: horizontal distance, the observed height difference, and the height difference a uniform slope predicts (d·tan slope), and the observed gradient |Δh| / d in metres per km.\n\n| Slope | Pairs | Distance P50 | Distance P90 | Observed P50 | Observed P90 | Predicted P50 | Predicted P90 | Gradient P50 | Gradient P90 |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n"
    );
    let mut lo = 0.0;
    for (hi, band) in SLOPE_BANDS.iter().zip(&mut bands) {
        for v in [
            &mut band.metres,
            &mut band.observed,
            &mut band.expected,
            &mut band.gradient,
        ] {
            v.sort_unstable_by(f32::total_cmp);
        }
        let (d, o, e, g) = (&band.metres, &band.observed, &band.expected, &band.gradient);
        let _ = writeln!(
            out,
            "| {lo:.0}–{hi:.0}° | {} | {:.0} m | {:.0} m | {:.1} | {:.1} | {:.1} | {:.1} | {:.0} | {:.0} |",
            d.len(),
            quantile(d, 0.5),
            quantile(d, 0.9),
            quantile(o, 0.5),
            quantile(o, 0.9),
            quantile(e, 0.5),
            quantile(e, 0.9),
            quantile(g, 0.5),
            quantile(g, 0.9)
        );
        lo = *hi;
    }
    Ok(())
}

/// Distances and height differences of the links in one slope band.
#[derive(Default)]
struct Banded {
    metres: Vec<f32>,
    observed: Vec<f32>,
    expected: Vec<f32>,
    gradient: Vec<f32>,
}

/// The nearest-rank quantile of sorted values (NaN when empty).
fn quantile(sorted: &[f32], q: f64) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an index"
    )]
    let k = ((q * sorted.len() as f64).ceil() as usize).max(1) - 1;
    sorted.get(k).copied().unwrap_or(f32::NAN)
}

/// Appends a table row: count, median signed, |Δ| P50/P90/P99, share above 50 m.
fn row(out: &mut String, label: &str, mut d: Vec<f32>) {
    d.sort_unstable_by(f32::total_cmp);
    let bias = quantile(&d, 0.5);
    let mut abs: Vec<f32> = d.iter().map(|x| x.abs()).collect();
    abs.sort_unstable_by(f32::total_cmp);
    let over = abs.iter().filter(|&&x| x > 50.0).count();
    #[allow(clippy::cast_precision_loss, reason = "counts")]
    let share = 100.0 * over as f64 / abs.len().max(1) as f64;
    let _ = writeln!(
        out,
        "{label}| {} | {bias:+.1} | {:.1} | {:.1} | {:.1} | {share:.1}% |",
        abs.len(),
        quantile(&abs, 0.5),
        quantile(&abs, 0.9),
        quantile(&abs, 0.99)
    );
}

fn write_source(dir: &Path, name: &str, recs: &[Rec], sampled: &[Sample]) -> Result<(), String> {
    let rows: Vec<(&Rec, &Sample)> = recs.iter().zip(sampled).collect();
    let f = |n: &str| Field::new(n, DataType::Float32, true);
    write_table(
        &dir.join(format!("{name}.parquet")),
        vec![
            Field::new("id", DataType::Utf8, false),
            f("dem_m"),
            f("slope_deg"),
            f("stated_m"),
            f("source_dem_m"),
        ],
        &rows,
        |r| {
            vec![
                strings(r.iter().map(|x| x.0.id.as_str())),
                f32s(r.iter().map(|x| x.1.map(|v| v.0))),
                f32s(r.iter().map(|x| x.1.map(|v| v.1))),
                f32s(r.iter().map(|x| x.0.stated)),
                f32s(r.iter().map(|x| x.0.source_dem)),
            ]
        },
    )
}

#[cfg(test)]
mod tests {
    use super::Grid;

    #[test]
    fn samples_bilinear_and_slope() {
        // A 4×4 plane rising 10 m per pixel eastwards, pixel centres on whole degrees / 3600.
        let dx = 1.0 / 3600.0;
        let z: Vec<f32> = (0..16)
            .map(|k| f32::from(u8::try_from(k % 4).unwrap_or(0)) * 10.0)
            .collect();
        let g = Grid {
            lon0: 85.0,
            lat0: 28.0,
            dx,
            dy: dx,
            w: 4,
            h: 4,
            z,
        };
        let (h, slope) = g
            .sample(85.0 + 1.5 * dx, 28.0 - 1.0 * dx)
            .unwrap_or((f32::NAN, f32::NAN));
        assert!((h - 15.0).abs() < 1e-3, "{h}");
        // 10 m per 1″ of longitude at 28° N (about 27.3 m): atan(10 / 27.3) ≈ 20°.
        let expect = (10.0 / (dx * super::M_PER_DEG * 28.0_f64.to_radians().cos()))
            .atan()
            .to_degrees();
        assert!(
            (f64::from(slope) - expect).abs() < 0.1,
            "{slope} vs {expect}"
        );
        assert!(g.sample(84.9, 28.0).is_none(), "west of the tile");
    }
}
