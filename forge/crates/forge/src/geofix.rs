//! The `geofix` detector stage (forge note §S5, metric 2): flags points that were rounded to
//! a grid, stacked on one coordinate, or snapped to an admin unit's centroid, and measures each
//! rate against a null with the same precision and spatial spread, never against zero.
//!
//! - **Decimals:** the source's native precision `p` is the finest decimal count held by at
//!   least 1% of points. Under a uniform null at `p` decimals, a point has ≤ k decimals in both
//!   coordinates with probability 10^(−2(p−k)), exactly.
//! - **Minute grids** (1′, 0.1′, 1″): a coordinate is on a grid when it is within half the
//!   native step of a grid line. The null is the same test on a grid shifted by 0.3536 of its
//!   spacing (snapped to the native step), so precision and clustering cancel. Points with
//!   ≤ 2 decimals are excluded, since 0.1° and 0.01° fall on these grids too.
//! - **Stacking:** points whose coordinates other records of the source share exactly.
//! - **Centroid snap:** points within 50 m of a unit's area centroid, at every level of a
//!   boundary set; the null queries every point shifted by 0.02° in both axes.
//!
//! Output: `data/build/geofix/<points>/flags.parquet` and `summary.md`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use arrow_schema::{DataType, Field};
use rayon::prelude::*;

use crate::fetch::store_path;
use crate::lock::Lock;
use crate::read::boundaries;
use crate::record::{self, Point, bools, f32s, i32s, strings, write_table};

/// One full degree on the grid.
const DEG: i64 = 10_000_000;
/// The minute grids, as subdivisions of a degree: 1′, 0.1′, 1″.
const GRIDS: [(&str, i64); 3] = [("1′", 60), ("0.1′", 600), ("1″", 3600)];
/// The null grid's shift, as a fraction of its spacing: far from every simple ratio.
const NULL_SHIFT: f64 = 0.3536;
/// Snap radius in metres.
const SNAP_M: f64 = 50.0;
/// Centroid index cells: 0.01° (about 1.1 km), so a 50 m query reads at most 3×3 cells.
const SNAP_CELL: i32 = 100_000;
/// The null shift for snapping: 0.02° (about 2 km) in both axes.
const SNAP_NULL: i32 = 200_000;
/// Metres per degree of latitude (mean Earth radius 6,371,008.8 m).
const M_PER_DEG: f64 = 111_195.08;

/// Number of decimals of a grid value (0–7).
fn decimals(v: i32) -> u8 {
    let mut v = v.unsigned_abs();
    if v == 0 {
        return 0;
    }
    let mut d = 7;
    while d > 0 && v.is_multiple_of(10) {
        v /= 10;
        d -= 1;
    }
    d
}

/// Whether `v` (plus `shift`) lies within half the native step `step` of a line of the grid
/// with `per_deg` lines per degree. Exact integer arithmetic.
fn on_grid(v: i32, shift: i64, per_deg: i64, step: i64) -> bool {
    let r = ((i64::from(v) + shift) * per_deg).rem_euclid(DEG);
    r.min(DEG - r) * 2 <= per_deg * step
}

/// The null grid's shift for `per_deg`, snapped to the native step.
fn null_shift(per_deg: i64, step: i64) -> i64 {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "small values"
    )]
    let raw = (NULL_SHIFT * DEG as f64 / per_deg as f64 / step as f64).round() as i64;
    raw.max(1) * step
}

/// The Wilson score interval at 95%.
fn wilson(hits: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 0.0);
    }
    #[allow(clippy::cast_precision_loss, reason = "counts")]
    let (k, n) = (hits as f64, n as f64);
    let z2 = 1.959_964_f64.powi(2);
    let p = k / n;
    let centre = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let half = (z2.sqrt() * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt()) / (1.0 + z2 / n);
    (centre - half, centre + half)
}

/// A unit's centroid: (level, lon, lat on the grid).
type Centroid = (u8, f64, f64);

/// Unit centroids of every level, indexed by 0.01° cell.
struct Centroids {
    cells: HashMap<(i32, i32), Vec<Centroid>>,
    count: usize,
}

impl Centroids {
    fn build(zip: &Path) -> Result<Self, String> {
        let mut cells: HashMap<(i32, i32), Vec<Centroid>> = HashMap::new();
        let mut count = 0;
        for level in boundaries::levels(zip)? {
            for unit in boundaries::read(zip, level)? {
                if let Some((x, y)) = unit.polygon.centroid() {
                    #[allow(clippy::cast_possible_truncation, reason = "grid values fit i32")]
                    let key = (x as i32).div_euclid(SNAP_CELL);
                    #[allow(clippy::cast_possible_truncation, reason = "grid values fit i32")]
                    let key = (key, (y as i32).div_euclid(SNAP_CELL));
                    cells.entry(key).or_default().push((level, x, y));
                    count += 1;
                }
            }
        }
        Ok(Self { cells, count })
    }

    /// The nearest centroid within [`SNAP_M`]: (level, metres).
    fn nearest(&self, lon: i32, lat: i32) -> Option<(u8, f64)> {
        let (cx, cy) = (lon.div_euclid(SNAP_CELL), lat.div_euclid(SNAP_CELL));
        let cos = (f64::from(lat) * 1e-7).to_radians().cos();
        let mut best: Option<(u8, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &(level, x, y) in self.cells.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let ex = (x - f64::from(lon)) * 1e-7 * M_PER_DEG * cos;
                    let ey = (y - f64::from(lat)) * 1e-7 * M_PER_DEG;
                    let d = ex.hypot(ey);
                    if d <= SNAP_M && best.is_none_or(|(_, b)| d < b) {
                        best = Some((level, d));
                    }
                }
            }
        }
        best
    }
}

/// Per-point flags.
struct Flags {
    decimals: u8,
    grid: [bool; 3],
    null_grid: [bool; 3],
    stack: u32,
    snap: Option<(u8, f64)>,
    null_snap: bool,
}

/// `forge geofix <points-source> [<boundary-source>]`.
pub(crate) fn run(root: &Path, points: &str, boundary: Option<&str>) -> Result<String, String> {
    let t = Instant::now();
    let pts = record::read_points(
        &root
            .join("data/build/read")
            .join(points)
            .join("records.parquet"),
    )?;
    let n = pts.len();
    let centroids = match boundary {
        Some(b) => {
            let lock = Lock::read(&root.join("data/sources.lock"))?;
            Some(Centroids::build(&store_path(root, lock.get(b)?))?)
        }
        None => None,
    };
    // Native precision: the finest decimal count held by at least 1% of points.
    let mut hist = [0usize; 8];
    for Point { lon: x, lat: y, .. } in &pts {
        if let Some(h) = hist.get_mut(usize::from(decimals(*x).max(decimals(*y)))) {
            *h += 1;
        }
    }
    let p = (0..8u8)
        .rev()
        .find(|&k| hist.get(usize::from(k)).is_some_and(|&h| h * 100 >= n))
        .unwrap_or(7);
    let step = 10_i64.pow(7 - u32::from(p));
    let shifts = GRIDS.map(|(_, f)| null_shift(f, step));
    let stacks = stack_sizes(&pts);
    let flags: Vec<Flags> = pts
        .par_iter()
        .zip(&stacks)
        .map(|(Point { lon: x, lat: y, .. }, &stack)| {
            let d = decimals(*x).max(decimals(*y));
            let test = |i: usize, shift: i64| {
                GRIDS.get(i).zip(Some(shift)).is_some_and(|((_, f), s)| {
                    d > 2 && on_grid(*x, s, *f, step) && on_grid(*y, s, *f, step)
                })
            };
            let shift = |i: usize| shifts.get(i).copied().unwrap_or(0);
            Flags {
                decimals: d,
                grid: [test(0, 0), test(1, 0), test(2, 0)],
                null_grid: [test(0, shift(0)), test(1, shift(1)), test(2, shift(2))],
                stack,
                snap: centroids.as_ref().and_then(|c| c.nearest(*x, *y)),
                null_snap: centroids.as_ref().is_some_and(|c| {
                    c.nearest(x.saturating_add(SNAP_NULL), y.saturating_add(SNAP_NULL))
                        .is_some()
                }),
            }
        })
        .collect();
    let dir = root.join("data/build/geofix").join(points);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    write_flags(&dir, &pts, &flags)?;
    let summary = summarise(
        points,
        boundary,
        &hist,
        p,
        &flags,
        centroids.as_ref().map(|c| c.count),
    );
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    Ok(format!("{summary}\n{n} points in {:.1?}", t.elapsed()))
}

/// For each point, how many points of the source share its exact coordinates.
fn stack_sizes(pts: &[Point]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..pts.len()).collect();
    let key = |i: usize| pts.get(i).map(|p| (p.lon, p.lat));
    order.par_sort_unstable_by_key(|&i| key(i));
    let mut out = vec![1u32; pts.len()];
    for group in order.chunk_by(|&a, &b| key(a) == key(b)) {
        let size = u32::try_from(group.len()).unwrap_or(u32::MAX);
        for &i in group {
            if let Some(o) = out.get_mut(i) {
                *o = size;
            }
        }
    }
    out
}

fn write_flags(dir: &Path, pts: &[Point], flags: &[Flags]) -> Result<(), String> {
    let rows: Vec<(&str, &Flags)> = pts.iter().map(|p| p.id.as_str()).zip(flags).collect();
    write_table(
        &dir.join("flags.parquet"),
        vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("decimals", DataType::Int32, false),
            Field::new("grid_1min", DataType::Boolean, false),
            Field::new("grid_01min", DataType::Boolean, false),
            Field::new("grid_1sec", DataType::Boolean, false),
            Field::new("stack", DataType::Int32, false),
            Field::new("snap_level", DataType::Int32, true),
            Field::new("snap_m", DataType::Float32, true),
        ],
        &rows,
        |r| {
            #[allow(clippy::cast_possible_truncation, reason = "metres below 50")]
            let metres = |f: &Flags| f.snap.map(|(_, m)| m as f32);
            vec![
                strings(r.iter().map(|x| x.0)),
                i32s(r.iter().map(|x| Some(i32::from(x.1.decimals)))),
                bools(r.iter().map(|x| x.1.grid[0])),
                bools(r.iter().map(|x| x.1.grid[1])),
                bools(r.iter().map(|x| x.1.grid[2])),
                i32s(
                    r.iter()
                        .map(|x| Some(i32::try_from(x.1.stack).unwrap_or(i32::MAX))),
                ),
                i32s(r.iter().map(|x| x.1.snap.map(|(l, _)| i32::from(l)))),
                f32s(r.iter().map(|x| metres(x.1))),
            ]
        },
    )
}

/// A table row: observed rate with its interval, the null, and the excess.
fn row(out: &mut String, label: &str, hits: usize, null: f64, n: usize) {
    #[allow(clippy::cast_precision_loss, reason = "counts")]
    let rate = if n == 0 { 0.0 } else { hits as f64 / n as f64 };
    let (lo, hi) = wilson(hits, n);
    let pct = |v: f64| format!("{:.2}%", v * 100.0);
    let _ = writeln!(
        out,
        "| {label} | {hits} | {} ({}–{}) | {} | {} |",
        pct(rate),
        pct(lo),
        pct(hi),
        pct(null),
        pct(rate - null)
    );
}

fn summarise(
    points: &str,
    boundary: Option<&str>,
    hist: &[usize; 8],
    p: u8,
    flags: &[Flags],
    centroids: Option<usize>,
) -> String {
    let n = flags.len();
    #[allow(clippy::cast_precision_loss, reason = "counts")]
    let share = |k: usize| if n == 0 { 0.0 } else { k as f64 / n as f64 };
    let mut out = format!(
        "## geofix: {points}\n\n{n} points; native precision {p} decimals. Decimals held (finer of lat, lon): {}\n\n| Test | Points | Rate (95% CI) | Null | Excess |\n|---|---:|---:|---:|---:|\n",
        hist.iter()
            .enumerate()
            .map(|(k, h)| format!("{k}: {h}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for k in (0..p).rev().take(3) {
        let hits = flags.iter().filter(|f| f.decimals <= k).count();
        let null = 10f64.powi(-2 * i32::from(p - k));
        row(&mut out, &format!("≤ {k} decimals"), hits, null, n);
    }
    for (i, (name, _)) in GRIDS.iter().enumerate() {
        let hits = flags
            .iter()
            .filter(|f| f.grid.get(i) == Some(&true))
            .count();
        let null = flags
            .iter()
            .filter(|f| f.null_grid.get(i) == Some(&true))
            .count();
        row(
            &mut out,
            &format!("on the {name} grid (> 2 decimals)"),
            hits,
            share(null),
            n,
        );
    }
    for min in [2, 5] {
        let hits = flags.iter().filter(|f| f.stack >= min).count();
        row(
            &mut out,
            &format!("stacked, ≥ {min} share a point"),
            hits,
            0.0,
            n,
        );
    }
    if let (Some(b), Some(c)) = (boundary, centroids) {
        let hits = flags.iter().filter(|f| f.snap.is_some()).count();
        let null = flags.iter().filter(|f| f.null_snap).count();
        row(
            &mut out,
            &format!("within {SNAP_M} m of a {b} centroid ({c} units)"),
            hits,
            share(null),
            n,
        );
    }
    out.push_str("\nThe stacking null is 0: distinct places rarely share a point to 1e-7°.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::{Point, decimals, null_shift, on_grid, stack_sizes, wilson};

    #[test]
    fn decimals_and_grids() {
        assert_eq!(decimals(277_016_900), 5);
        assert_eq!(decimals(270_000_000), 0);
        assert_eq!(decimals(-1), 7);
        assert_eq!(decimals(0), 0);
        // 27°42′ = 27.7°: on the 1′ grid at any precision.
        assert!(on_grid(277_000_000, 0, 60, 1));
        // 27°42′30″ = 27.708333…; at 5 decimals 27.70833 is on the 1″ grid, not the 1′ grid.
        assert!(on_grid(277_083_300, 0, 3600, 100));
        assert!(!on_grid(277_083_300, 0, 60, 100));
        // At 7 decimals the same rounded value is still within half a step of the line.
        assert!(on_grid(277_083_333, 0, 3600, 1));
        assert!(!on_grid(277_083_340, 0, 3600, 1));
        // The null shift moves a grid point off the grid.
        let s = null_shift(60, 100);
        assert_eq!(s % 100, 0);
        assert!(!on_grid(277_000_000, s, 60, 100));
    }

    #[test]
    fn stacks_and_intervals() {
        let p = |lon, lat| Point {
            lon,
            lat,
            ..Point::default()
        };
        assert_eq!(
            stack_sizes(&[p(1, 1), p(2, 2), p(1, 1), p(1, 1)]),
            vec![3, 1, 3, 3]
        );
        let (lo, hi) = wilson(50, 100);
        assert!((lo - 0.4038).abs() < 1e-3 && (hi - 0.5962).abs() < 1e-3);
        assert_eq!(wilson(0, 0), (0.0, 0.0));
    }
}
