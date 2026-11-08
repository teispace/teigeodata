//! The `core` stage: each conflated place's consensus point, with the settlement core from
//! buildings as one more observation (D-091,
//! [settlement-core](../../../../docs/02-research/notes/settlement-core.md) §3.4).
//!
//! ```text
//! forge core <points-source>+<points-source>… <buildings-source>+<buildings-source>…
//! ```
//!
//! For every cluster of the conflation run that holds a populated place or a locality:
//! 1. **Seed:** the weighted geometric median (Weiszfeld, Vardi–Zhang, 64 iterations, in a
//!    local plane in metres) of its members' points. A point's σ is the larger of the source
//!    prior and its precision floor, step/√12 for its decimals (D-055), and its weight is 1/σ.
//!    Points of different sources within 1 m are one copied observation (D-056). With two
//!    observations the median is a whole segment, so the heavier one wins (then the earlier
//!    source).
//! 2. **Assignment cells:** every building belongs to the nearest populated-place seed (ties to
//!    the lower cluster index). Localities own no buildings.
//! 3. **Core:** inside its own cell only, the buildings within the class gate plus three
//!    bandwidths of the seed are counted on a 25 m grid, blurred by a separable Gaussian of
//!    the class bandwidth, and climbed by steepest 8-neighbour ascent from the seed's cell
//!    (ties to the lower row, then column). Everything is integer or IEEE arithmetic in a
//!    fixed order, with our own `exp`, so the bytes do not depend on the platform's libm.
//! 4. **Gates:** the core is kept only with at least [`MIN_BUILDINGS`] buildings in the window,
//!    a density at the seed, a mode at least half the window's densest, and within the class
//!    gate of the seed. Otherwise the reason is recorded. Cities get no core: their centre is
//!    not where buildings are densest.
//! 5. **Consensus:** the median again, with the core as an observation of σ [`SIGMA_CORE_M`].
//!
//! With buildings from both Google and Microsoft, the cores from each alone are compared: the
//! agreement gate of the note (P90 ≤ 180 m on Nepal) is reported in the summary. A failed gate
//! is an error and leaves no points, so no later stage reads them.
//!
//! Output: `data/build/core/<points run>/<buildings run>/points.parquet` (`cluster`, `lon_e7`,
//! `lat_e7`, `seed_lon_e7`, `seed_lat_e7`, `core_lon_e7`, `core_lat_e7`, `status`,
//! `buildings`, `sigma_m`) and `summary.md`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use arrow_array::{Array as _, ArrayRef, Float32Array, Int32Array, StringArray};
use arrow_schema::{DataType, Field};
use rayon::prelude::*;

use crate::buildings::Pt;
use crate::num::cos_deg;
use crate::record::{column, for_each_batch, i32s, strings, write_table};

/// Metres per degree of latitude (mean Earth radius 6,371,008.8 m).
const M_PER_DEG: f64 = 111_195.08;
/// Grid cell of the core raster, metres.
const CELL_M: f64 = 25.0;
/// The prior σ of a source point, metres per axis (**estimate**: independent sources disagree
/// P50 360–1,100 m on villages, D-056). Fitted on the golden set before release.
const SIGMA_SOURCE_M: f64 = 300.0;
/// The σ of an accepted core, metres per axis (**estimate**, settlement-core §3.4).
const SIGMA_CORE_M: f64 = 75.0;
/// Fewest buildings in the window for a core (**estimate**).
const MIN_BUILDINGS: u32 = 5;
/// A mode must reach this share of the window's densest cell (settlement-core §3.4).
const MODE_SHARE: f32 = 0.5;
/// Weiszfeld iterations (fixed, so the result is deterministic).
const ITERATIONS: usize = 64;
/// Two observations of different sources this close are one copied observation (D-056).
const COPY_M: f64 = 1.0;
/// Seed index cells, metres.
const SEED_CELL_M: f64 = 2_000.0;

/// The scale a place's buildings are read at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class {
    Hamlet,
    Village,
    Town,
    City,
}

impl Class {
    /// Bandwidth (σ of the kernel) and gate, metres (**estimates**, settlement-core §3.2).
    fn scale(self) -> (f64, f64) {
        match self {
            Self::Hamlet => (75.0, 500.0),
            Self::Village => (150.0, 1_000.0),
            Self::Town | Self::City => (300.0, 2_000.0),
        }
    }

    /// The class one source class says, if it says one.
    fn of(class: &str) -> Option<Self> {
        Some(match class {
            "place=city" | "P.PPLC" => Self::City,
            "place=town" | "P.PPLA" | "P.PPLA2" | "P.PPLA3" | "P.PPLA4" => Self::Town,
            "place=hamlet" | "place=isolated_dwelling" | "place=farm" => Self::Hamlet,
            "place=village"
            | "place=suburb"
            | "place=neighbourhood"
            | "place=quarter"
            | "P.PPL"
            | "P.PPLL" => Self::Village,
            _ => return None,
        })
    }
}

/// A deterministic e^x for x ≤ 0 from basic IEEE operations only: x/2^k in [−1/16, 0], a
/// degree-12 Taylor polynomial (truncation below 1e-28 there), squared k times (relative
/// error about 2^k ulp, below 1e-12 for any kernel weight).
fn exp_neg(x: f64) -> f64 {
    if x < -700.0 {
        return 0.0;
    }
    let mut k = 0;
    let mut r = x;
    while r < -0.0625 {
        r *= 0.5;
        k += 1;
    }
    let mut term = 1.0;
    let mut sum = 1.0;
    for i in 1..=12_u8 {
        term *= r / f64::from(i);
        sum += term;
    }
    for _ in 0..k {
        sum *= sum;
    }
    sum
}

/// One observation in a local plane: (east, north) in metres, weight.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Obs {
    x: f64,
    y: f64,
    w: f64,
}

/// The weighted geometric median (Weiszfeld with the Vardi–Zhang step at data points),
/// started at the weighted coordinate-wise median.
#[allow(
    clippy::many_single_char_names,
    reason = "the iteration's usual symbols"
)]
fn median(obs: &[Obs]) -> Option<(f64, f64)> {
    match obs {
        [] => return None,
        [o] => return Some((o.x, o.y)),
        // A segment: every point on it is a median, so the heavier end (then the first).
        [a, b] => return Some(if b.w > a.w { (b.x, b.y) } else { (a.x, a.y) }),
        _ => {}
    }
    let wmed = |key: fn(&Obs) -> f64| {
        let mut v: Vec<(f64, f64)> = obs.iter().map(|o| (key(o), o.w)).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        let half = v.iter().map(|p| p.1).sum::<f64>() / 2.0;
        let mut acc = 0.0;
        for (val, w) in &v {
            acc += w;
            if acc >= half {
                return *val;
            }
        }
        v.last().map_or(0.0, |p| p.0)
    };
    let (mut x, mut y) = (wmed(|o| o.x), wmed(|o| o.y));
    for _ in 0..ITERATIONS {
        let (mut sx, mut sy, mut sw, mut eta) = (0.0, 0.0, 0.0, 0.0);
        let (mut rx, mut ry) = (0.0, 0.0);
        for o in obs {
            let d = (o.x - x).hypot(o.y - y);
            if d < 1e-9 {
                eta += o.w;
                continue;
            }
            sx += o.w * o.x / d;
            sy += o.w * o.y / d;
            sw += o.w / d;
            rx += o.w * (o.x - x) / d;
            ry += o.w * (o.y - y) / d;
        }
        if sw == 0.0 {
            break;
        }
        let (tx, ty) = (sx / sw, sy / sw);
        let r = rx.hypot(ry);
        if eta == 0.0 || r == 0.0 {
            (x, y) = (tx, ty);
        } else {
            // Vardi & Zhang (2000): stay at a data point that is already the median.
            let a = (1.0 - eta / r).max(0.0);
            let b = (eta / r).min(1.0);
            (x, y) = (a * tx + b * x, a * ty + b * y);
        }
    }
    Some((x, y))
}

/// A local plane around (lon0, lat0) on the 1e-7° grid: metres east and north.
#[derive(Clone, Copy, Debug)]
struct Plane {
    lon0: i32,
    lat0: i32,
    kx: f64,
}

impl Plane {
    fn at(lon0: i32, lat0: i32) -> Self {
        let lat = f64::from(lat0) * 1e-7;
        // cos from our own series is not needed: one cos per place, and its last bit cannot
        // move a 1e-7° output except at exact ties, which the snap below resolves the same way
        // on IEEE platforms that round correctly. Use the deterministic polynomial anyway.
        Self {
            lon0,
            lat0,
            kx: M_PER_DEG * cos_deg(lat),
        }
    }

    fn to_m(self, (lon, lat): Pt) -> (f64, f64) {
        (
            f64::from(lon - self.lon0) * 1e-7 * self.kx,
            f64::from(lat - self.lat0) * 1e-7 * M_PER_DEG,
        )
    }

    /// Back to the grid, rounded half away from zero.
    fn to_grid(self, (x, y): (f64, f64)) -> Option<Pt> {
        let lon = f64::from(self.lon0) + (x / self.kx) * 1e7;
        let lat = f64::from(self.lat0) + (y / M_PER_DEG) * 1e7;
        let ok = |v: f64| v.is_finite() && v.abs() <= 1.8e9;
        #[allow(clippy::cast_possible_truncation, reason = "range checked")]
        (ok(lon) && ok(lat)).then(|| (lon.round() as i32, lat.round() as i32))
    }
}

/// σ floor of a coordinate from its decimals (D-055), metres.
fn precision_m(lon: i32, lat: i32) -> f64 {
    let dec = |v: i32| {
        let mut v = v.unsigned_abs();
        let mut d = 7;
        while d > 0 && v != 0 && v.is_multiple_of(10) {
            v /= 10;
            d -= 1;
        }
        d
    };
    let d = dec(lon).max(dec(lat));
    let step = M_PER_DEG * 10_f64.powi(-d);
    step / 12_f64.sqrt()
}

/// A place of the run: its members' points in source order.
struct Place {
    cluster: String,
    class: Class,
    /// Owns buildings (a populated place, not a locality).
    settled: bool,
    /// (source rank, point).
    points: Vec<(usize, Pt)>,
}

/// What the core gave for one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Core,
    NoPlace,
    City,
    NoBuildings,
    Few,
    NoDensity,
    Weak,
    Far,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::NoPlace => "locality",
            Self::City => "city",
            Self::NoBuildings => "no-buildings",
            Self::Few => "few-buildings",
            Self::NoDensity => "no-density-at-seed",
            Self::Weak => "weak-mode",
            Self::Far => "beyond-gate",
        }
    }
}

/// A square window of 25 m cells centred on the seed: `half` cells each side.
#[derive(Clone, Copy)]
struct Window {
    half: u32,
}

impl Window {
    fn side(self) -> usize {
        2 * self.half as usize + 1
    }

    /// The cell index of a coordinate in metres from the seed, if inside.
    fn cell(self, v: f64) -> Option<usize> {
        let i = (v / CELL_M + 0.5).floor() + f64::from(self.half);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked against the window"
        )]
        (i >= 0.0 && i < f64::from(2 * self.half + 1)).then_some(i as usize)
    }

    /// The centre of a cell, metres from the seed.
    fn back(self, i: usize) -> f64 {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        (f64::from(i) - f64::from(self.half)) * CELL_M
    }
}

/// A separable Gaussian blur of σ = `h` metres, kernel truncated at 3σ: rows, then columns,
/// in a fixed order. Only the occupied box grown by the kernel radius is computed: every other
/// cell is exactly zero, and skipped terms are exact zeros, so the result is the same bits as
/// the full blur.
#[allow(
    clippy::many_single_char_names,
    reason = "box corners and kernel symbols"
)]
fn blur(counts: &[u32], side: usize, h: f64) -> Vec<f32> {
    let s = h / CELL_M;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few dozen taps"
    )]
    let r = (3.0 * s).ceil() as u32;
    #[allow(clippy::cast_possible_truncation, reason = "a kernel weight in (0, 1]")]
    let kernel: Vec<f32> = (0..=r)
        .map(|k| {
            let k = f64::from(k);
            exp_neg(-(k * k) / (2.0 * s * s)) as f32
        })
        .collect();
    let r = r as usize;
    // The occupied box.
    let (mut x0, mut y0, mut x1, mut y1) = (side, side, 0, 0);
    for (i, _) in counts.iter().enumerate().filter(|c| *c.1 > 0) {
        let (x, y) = (i % side, i / side);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let mut out = vec![0.0_f32; counts.len()];
    if x0 > x1 {
        return out;
    }
    let grow = |lo: usize, hi: usize| (lo.saturating_sub(r), (hi + r).min(side - 1));
    let ((gx0, gx1), (gy0, gy1)) = (grow(x0, x1), grow(y0, y1));
    // One 1-D pass at index i of a line read through `at`.
    let tap = |at: &dyn Fn(usize) -> f32, i: usize| -> f32 {
        let mut acc = 0.0_f32;
        for (k, w) in kernel.iter().enumerate() {
            if k == 0 {
                acc += w * at(i);
                continue;
            }
            if let Some(j) = i.checked_sub(k) {
                acc += w * at(j);
            }
            if i + k < side {
                acc += w * at(i + k);
            }
        }
        acc
    };
    #[allow(clippy::cast_precision_loss, reason = "counts are far below 2^24")]
    let count = |x: usize, y: usize| counts.get(y * side + x).map_or(0.0, |&c| c as f32);
    // Rows: only occupied rows have anything to blur.
    let mut rows = vec![0.0_f32; counts.len()];
    for y in y0..=y1 {
        for x in gx0..=gx1 {
            if let Some(o) = rows.get_mut(y * side + x) {
                *o = tap(&|i| count(i, y), x);
            }
        }
    }
    // Columns.
    for x in gx0..=gx1 {
        for y in gy0..=gy1 {
            let v = tap(&|i| rows.get(i * side + x).copied().unwrap_or(0.0), y);
            if let Some(o) = out.get_mut(y * side + x) {
                *o = v;
            }
        }
    }
    out
}

/// Steepest 8-neighbour ascent from `start` (column, row); ties go to the lower (row, column),
/// so the climb is the same on every platform. Bounded by the window size.
#[allow(
    clippy::float_cmp,
    reason = "exact ties are broken by position on purpose"
)]
fn ascend(dens: &[f32], side: usize, start: (usize, usize)) -> (usize, usize) {
    let at = |cx: usize, cy: usize| dens.get(cy * side + cx).copied().unwrap_or(0.0);
    let (mut cx, mut cy) = start;
    for _ in 0..side * side {
        let mut best = (at(cx, cy), cy, cx);
        for dy in [-1_isize, 0, 1] {
            for dx in [-1_isize, 0, 1] {
                let (Some(nx), Some(ny)) = (cx.checked_add_signed(dx), cy.checked_add_signed(dy))
                else {
                    continue;
                };
                if nx >= side || ny >= side {
                    continue;
                }
                let v = at(nx, ny);
                if v > best.0 || (v == best.0 && (ny, nx) < (best.1, best.2)) {
                    best = (v, ny, nx);
                }
            }
        }
        if (best.1, best.2) == (cy, cx) {
            break;
        }
        (cy, cx) = (best.1, best.2);
    }
    (cx, cy)
}

/// The core of one place from its own buildings (in its plane): status, core (metres), and
/// the number of buildings in the window.
fn core_of(own: &[(f64, f64)], class: Class) -> (Status, Option<(f64, f64)>, u32) {
    let (h, gate) = class.scale();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few hundred cells"
    )]
    let win = Window {
        half: ((gate + 3.0 * h) / CELL_M).ceil() as u32,
    };
    let side = win.side();
    let mut counts = vec![0_u32; side * side];
    let mut n = 0_u32;
    for &(x, y) in own {
        if let (Some(cx), Some(cy)) = (win.cell(x), win.cell(y))
            && let Some(c) = counts.get_mut(cy * side + cx)
        {
            *c = c.saturating_add(1);
            n += 1;
        }
    }
    if n == 0 {
        return (Status::NoBuildings, None, 0);
    }
    if n < MIN_BUILDINGS {
        return (Status::Few, None, n);
    }
    let dens = blur(&counts, side, h);
    let mid = win.half as usize;
    if dens.get(mid * side + mid).is_none_or(|&v| v <= 0.0) {
        return (Status::NoDensity, None, n);
    }
    let (cx, cy) = ascend(&dens, side, (mid, mid));
    let peak = dens.iter().copied().fold(0.0_f32, f32::max);
    let core = (win.back(cx), win.back(cy));
    if dens
        .get(cy * side + cx)
        .is_none_or(|&v| v < MODE_SHARE * peak)
    {
        return (Status::Weak, Some(core), n);
    }
    if core.0.hypot(core.1) > gate {
        return (Status::Far, Some(core), n);
    }
    (Status::Core, Some(core), n)
}

/// A uniform grid over the seeds of the populated places, for nearest-seed queries.
struct SeedGrid {
    /// Seeds in grid units of [`SEED_CELL_M`] (east, north) in the run's plane.
    cells: BTreeMap<(i64, i64), Vec<u32>>,
    seeds: Vec<(f64, f64)>,
}

impl SeedGrid {
    fn key(p: (f64, f64)) -> (i64, i64) {
        #[allow(clippy::cast_possible_truncation, reason = "metres within a country")]
        (
            (p.0 / SEED_CELL_M).floor() as i64,
            (p.1 / SEED_CELL_M).floor() as i64,
        )
    }

    fn new(seeds: Vec<(f64, f64)>) -> Self {
        let mut cells: BTreeMap<(i64, i64), Vec<u32>> = BTreeMap::new();
        for (i, &s) in seeds.iter().enumerate() {
            cells
                .entry(Self::key(s))
                .or_default()
                .push(u32::try_from(i).unwrap_or(u32::MAX));
        }
        Self { cells, seeds }
    }

    /// The nearest seed within `max_m`, ties to the lower index.
    #[allow(
        clippy::float_cmp,
        reason = "exact ties are broken by index on purpose"
    )]
    fn nearest(&self, p: (f64, f64), max_m: f64) -> Option<u32> {
        let (kx, ky) = Self::key(p);
        #[allow(clippy::cast_possible_truncation, reason = "a few rings")]
        let rings = (max_m / SEED_CELL_M).ceil() as i32 + 1;
        let mut best: Option<(f64, u32)> = None;
        for ring in 0..=rings {
            // Every seed in ring k is at least (k − 1) cells away.
            if let Some((d, _)) = best
                && d <= (f64::from(ring - 1) * SEED_CELL_M).powi(2)
            {
                break;
            }
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let key = (kx + i64::from(dx), ky + i64::from(dy));
                    for &i in self.cells.get(&key).into_iter().flatten() {
                        let Some(s) = self.seeds.get(i as usize) else {
                            continue;
                        };
                        let d = (s.0 - p.0).powi(2) + (s.1 - p.1).powi(2);
                        if best.is_none_or(|(bd, bi)| d < bd || (d == bd && i < bi)) {
                            best = Some((d, i));
                        }
                    }
                }
            }
        }
        best.filter(|(d, _)| *d <= max_m * max_m).map(|(_, i)| i)
    }
}

/// Reads the run's places: clusters with a populated place or a locality, members in source
/// order (`sources`), each member's point and class.
fn places(build: &Path, sources: &[String]) -> Result<Vec<Place>, String> {
    // (source rank, point, class, kind) by record ID.
    let mut recs: BTreeMap<String, (usize, Option<Pt>, String, String)> = BTreeMap::new();
    for (rank, s) in sources.iter().enumerate() {
        for_each_batch(&build.join("read").join(s).join("records.parquet"), |b| {
            let id = column::<StringArray>(b, "id")?;
            let class = column::<StringArray>(b, "class")?;
            let kind = column::<StringArray>(b, "kind")?;
            let lat = column::<Int32Array>(b, "lat_e7")?;
            let lon = column::<Int32Array>(b, "lon_e7")?;
            for i in 0..b.num_rows() {
                let pt = (!lat.is_null(i) && !lon.is_null(i)).then(|| (lon.value(i), lat.value(i)));
                recs.insert(
                    id.value(i).to_string(),
                    (
                        rank,
                        pt,
                        class.value(i).to_string(),
                        kind.value(i).to_string(),
                    ),
                );
            }
            Ok(())
        })?;
    }
    let mut clusters: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let path = build
        .join("conflate")
        .join(sources.join("+"))
        .join("clusters.parquet");
    for_each_batch(&path, |b| {
        let id = column::<StringArray>(b, "id")?;
        let cl = column::<StringArray>(b, "cluster")?;
        for i in 0..b.num_rows() {
            clusters
                .entry(cl.value(i).to_string())
                .or_default()
                .push(id.value(i).to_string());
        }
        Ok(())
    })?;
    let mut out = Vec::new();
    for (cluster, ids) in clusters {
        let members: Vec<&(usize, Option<Pt>, String, String)> =
            ids.iter().filter_map(|id| recs.get(id)).collect();
        let settled = members.iter().any(|m| m.3 == "populated_place");
        if !settled && !members.iter().any(|m| m.3 == "locality") {
            continue;
        }
        let mut points: Vec<(usize, Pt)> = members
            .iter()
            .filter_map(|m| m.1.map(|p| (m.0, p)))
            .collect();
        points.sort_unstable();
        if points.is_empty() {
            continue;
        }
        // The class: OSM's place tag first, then GeoNames' feature code, else a village.
        let class = ["place=", "P."]
            .iter()
            .find_map(|pre| {
                members
                    .iter()
                    .filter(|m| m.2.starts_with(pre))
                    .filter_map(|m| Class::of(&m.2))
                    .max()
            })
            .unwrap_or(Class::Village);
        out.push(Place {
            cluster,
            class,
            settled,
            points,
        });
    }
    Ok(out)
}

/// The observations of a place's members in its plane, copies merged.
fn observations(plane: Plane, points: &[(usize, Pt)]) -> Vec<Obs> {
    let mut obs: Vec<(usize, Obs)> = Vec::new();
    for &(rank, p) in points {
        let (x, y) = plane.to_m(p);
        let w = 1.0 / SIGMA_SOURCE_M.max(precision_m(p.0, p.1));
        // A point of another source within 1 m is the same observation copied: keep one.
        if let Some(o) = obs
            .iter_mut()
            .find(|(r, o)| *r != rank && (o.x - x).hypot(o.y - y) < COPY_M)
        {
            o.1.w = o.1.w.max(w);
            continue;
        }
        obs.push((rank, Obs { x, y, w }));
    }
    obs.into_iter().map(|o| o.1).collect()
}

/// One place's result.
struct Out {
    point: Pt,
    seed: Pt,
    core: Option<Pt>,
    status: Status,
    buildings: u32,
    sigma_m: f32,
}

/// A run: the places, their seeds, and the buildings each place owns per building source.
struct Run {
    places: Vec<Place>,
    seeds: Vec<Pt>,
    /// `owned[source][place]`: the place's buildings from that source.
    owned: Vec<Vec<Vec<Pt>>>,
    /// Per building source: (ID, buildings, buildings in some place's cell).
    counts: Vec<(String, usize, usize)>,
}

impl Run {
    fn new(build: &Path, sources: &[String], bsources: &[String]) -> Result<Self, String> {
        let places = places(build, sources)?;
        if places.is_empty() {
            return Err(format!(
                "{}: no populated places in the conflation",
                sources.join("+")
            ));
        }
        let seeds: Vec<Pt> = places
            .par_iter()
            .map(|p| {
                let first = p.points.first().map_or((0, 0), |q| q.1);
                let plane = Plane::at(first.0, first.1);
                median(&observations(plane, &p.points))
                    .and_then(|m| plane.to_grid(m))
                    .unwrap_or(first)
            })
            .collect();
        let mut run = Self {
            places,
            seeds,
            owned: Vec::new(),
            counts: Vec::new(),
        };
        run.assign(build, bsources)?;
        Ok(run)
    }

    /// Gives every building to the nearest populated-place seed within reach.
    fn assign(&mut self, build: &Path, bsources: &[String]) -> Result<(), String> {
        // One plane for the cells: the median of the seeds (metres stay exact enough over a
        // country; the planet build cuts tiles first).
        let mut lons: Vec<i32> = self.seeds.iter().map(|s| s.0).collect();
        let mut lats: Vec<i32> = self.seeds.iter().map(|s| s.1).collect();
        lons.sort_unstable();
        lats.sort_unstable();
        let mid = |v: &[i32]| v.get(v.len() / 2).copied().unwrap_or(0);
        let plane = Plane::at(mid(&lons), mid(&lats));
        let settled: Vec<u32> = (0..self.places.len())
            .filter(|&i| self.places.get(i).is_some_and(|p| p.settled))
            .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
            .collect();
        let grid = SeedGrid::new(
            settled
                .iter()
                .filter_map(|&i| self.seeds.get(i as usize).map(|&s| plane.to_m(s)))
                .collect(),
        );
        let (h, gate) = Class::City.scale();
        let reach = gate + 3.0 * h;
        for s in bsources {
            let mut pts: Vec<Pt> = Vec::new();
            for_each_batch(
                &build.join("buildings").join(s).join("points.parquet"),
                |b| {
                    let lon = column::<Int32Array>(b, "lon_e7")?;
                    let lat = column::<Int32Array>(b, "lat_e7")?;
                    pts.extend((0..b.num_rows()).map(|i| (lon.value(i), lat.value(i))));
                    Ok(())
                },
            )?;
            let nearest: Vec<Option<u32>> = pts
                .par_iter()
                .map(|&p| grid.nearest(plane.to_m(p), reach))
                .collect();
            let mut by_place: Vec<Vec<Pt>> = vec![Vec::new(); self.places.len()];
            for (p, o) in pts.iter().zip(&nearest) {
                if let Some(&i) = o.and_then(|o| settled.get(o as usize))
                    && let Some(v) = by_place.get_mut(i as usize)
                {
                    v.push(*p);
                }
            }
            let inside = nearest.iter().filter(|o| o.is_some()).count();
            self.counts.push((s.clone(), pts.len(), inside));
            self.owned.push(by_place);
        }
        Ok(())
    }

    /// Core and consensus of place `i` from the building sources `set`.
    fn one(&self, i: usize, set: &[usize]) -> Out {
        let (Some(p), Some(&seed)) = (self.places.get(i), self.seeds.get(i)) else {
            return Out {
                point: (0, 0),
                seed: (0, 0),
                core: None,
                status: Status::NoPlace,
                buildings: 0,
                sigma_m: 0.0,
            };
        };
        let plane = Plane::at(seed.0, seed.1);
        let mut obs = observations(plane, &p.points);
        let (status, core, n) = if !p.settled {
            (Status::NoPlace, None, 0)
        } else if p.class == Class::City {
            (Status::City, None, 0)
        } else {
            let own: Vec<(f64, f64)> = set
                .iter()
                .filter_map(|&s| self.owned.get(s).and_then(|o| o.get(i)))
                .flatten()
                .map(|&b| plane.to_m(b))
                .collect();
            core_of(&own, p.class)
        };
        let accepted = if status == Status::Core { core } else { None };
        if let Some(c) = accepted {
            obs.push(Obs {
                x: c.0,
                y: c.1,
                w: 1.0 / SIGMA_CORE_M,
            });
        }
        let point = median(&obs).and_then(|m| plane.to_grid(m)).unwrap_or(seed);
        // σ: the weighted median distance of the observations to the consensus, at least
        // the prior of the best observation.
        let (cx, cy) = plane.to_m(point);
        let mut d: Vec<(f64, f64)> = obs
            .iter()
            .map(|o| ((o.x - cx).hypot(o.y - cy), o.w))
            .collect();
        d.sort_by(|a, b| a.0.total_cmp(&b.0));
        let half = d.iter().map(|x| x.1).sum::<f64>() / 2.0;
        let mut acc = 0.0;
        let spread = d
            .iter()
            .find(|x| {
                acc += x.1;
                acc >= half
            })
            .map_or(0.0, |x| x.0);
        let floor = if accepted.is_some() {
            SIGMA_CORE_M
        } else {
            SIGMA_SOURCE_M
        };
        #[allow(clippy::cast_possible_truncation, reason = "metres")]
        let sigma_m = spread.max(floor) as f32;
        Out {
            point,
            seed,
            core: accepted.and_then(|c| plane.to_grid(c)),
            status,
            buildings: n,
            sigma_m,
        }
    }

    fn write(&self, path: &Path, outs: &[Out]) -> Result<(), String> {
        let rows: Vec<(usize, &Out)> = outs.iter().enumerate().collect();
        let ints = |f: &dyn Fn(&Out) -> i32, rows: &[(usize, &Out)]| -> ArrayRef {
            Arc::new(Int32Array::from_iter_values(rows.iter().map(|r| f(r.1))))
        };
        write_table(
            path,
            vec![
                Field::new("cluster", DataType::Utf8, false),
                Field::new("lon_e7", DataType::Int32, false),
                Field::new("lat_e7", DataType::Int32, false),
                Field::new("seed_lon_e7", DataType::Int32, false),
                Field::new("seed_lat_e7", DataType::Int32, false),
                Field::new("core_lon_e7", DataType::Int32, true),
                Field::new("core_lat_e7", DataType::Int32, true),
                Field::new("status", DataType::Utf8, false),
                Field::new("buildings", DataType::Int32, false),
                Field::new("sigma_m", DataType::Float32, false),
            ],
            &rows,
            |rows| {
                vec![
                    strings(
                        rows.iter()
                            .map(|r| self.places.get(r.0).map_or("", |p| p.cluster.as_str())),
                    ),
                    ints(&|o| o.point.0, rows),
                    ints(&|o| o.point.1, rows),
                    ints(&|o| o.seed.0, rows),
                    ints(&|o| o.seed.1, rows),
                    i32s(rows.iter().map(|r| r.1.core.map(|c| c.0))),
                    i32s(rows.iter().map(|r| r.1.core.map(|c| c.1))),
                    strings(rows.iter().map(|r| r.1.status.name())),
                    ints(&|o| i32::try_from(o.buildings).unwrap_or(i32::MAX), rows),
                    Arc::new(Float32Array::from_iter_values(
                        rows.iter().map(|r| r.1.sigma_m),
                    )),
                ]
            },
        )
    }

    /// The summary, and whether every gate passed.
    fn summarise(&self, outs: &[Out], bsources: &[String], title: &str) -> (String, bool) {
        let mut s = format!("# core: {title}\n\n");
        let mut by: BTreeMap<&str, usize> = BTreeMap::new();
        for o in outs {
            *by.entry(o.status.name()).or_default() += 1;
        }
        let _ = writeln!(s, "{} places; status: {by:?}.\n", self.places.len());
        for (src, n, inside) in &self.counts {
            let _ = writeln!(s, "- {src}: {n} buildings, {inside} in a place's cell.");
        }
        let mut moved: Vec<f64> = outs
            .iter()
            .filter(|o| o.status == Status::Core)
            .map(|o| dist_m(o.seed, o.point))
            .collect();
        let (p50, p90) = (quantile(&mut moved, 0.5), quantile(&mut moved, 0.9));
        let _ = writeln!(
            s,
            "\nPlaces with a core moved from their seed P50 {p50:.0} m, P90 {p90:.0} m (n = {}).",
            moved.len()
        );
        // The agreement gate: the core from each building source alone, where both have one.
        let gi = bsources.iter().position(|b| b.contains("google"));
        let mi = bsources.iter().position(|b| b.contains("msbuildings"));
        let mut pass = true;
        if let (Some(g), Some(m)) = (gi, mi) {
            let mut v: Vec<f64> = (0..self.places.len())
                .into_par_iter()
                .filter_map(|i| {
                    let (a, b) = (self.one(i, &[g]), self.one(i, &[m]));
                    a.core.zip(b.core).map(|(x, y)| dist_m(x, y))
                })
                .collect();
            let n = v.len();
            let (a50, a90) = (quantile(&mut v, 0.5), quantile(&mut v, 0.9));
            pass = a90 <= 180.0;
            let verdict = if pass { "pass" } else { "FAIL" };
            let _ = writeln!(
                s,
                "\nAgreement gate (Google alone against Microsoft alone, both with a core, n = {n}): P50 {a50:.0} m, P90 {a90:.0} m, limit P90 ≤ 180 m: **{verdict}**."
            );
        }
        (s, pass)
    }
}

/// `forge core <points run> <buildings run>`.
pub(crate) fn run(root: &Path, points_run: &str, buildings_run: &str) -> Result<String, String> {
    let t = Instant::now();
    let build = root.join("data/build");
    let sources: Vec<String> = points_run.split('+').map(str::to_string).collect();
    let bsources: Vec<String> = buildings_run.split('+').map(str::to_string).collect();
    let run = Run::new(&build, &sources, &bsources)?;
    let all: Vec<usize> = (0..bsources.len()).collect();
    let outs: Vec<Out> = (0..run.places.len())
        .into_par_iter()
        .map(|i| run.one(i, &all))
        .collect();
    let dir = build.join("core").join(points_run).join(buildings_run);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (mut summary, pass) = run.summarise(
        &outs,
        &bsources,
        &format!("{points_run} with {buildings_run}"),
    );
    let _ = writeln!(summary, "\nIn {:.1?}.", t.elapsed());
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    let points = dir.join("points.parquet");
    if !pass {
        // A failed gate leaves no output a later stage could read.
        let _ = fs::remove_file(&points);
        return Err(format!("{summary}\ncore: a gate failed; no points written"));
    }
    run.write(&points, &outs)?;
    Ok(summary)
}

/// Distance in metres between two grid points (local plane at the first).
fn dist_m(a: Pt, b: Pt) -> f64 {
    let (x, y) = Plane::at(a.0, a.1).to_m(b);
    x.hypot(y)
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let last = u32::try_from(v.len() - 1).unwrap_or(u32::MAX);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an index"
    )]
    let i = (f64::from(last) * q).round() as usize;
    v.get(i).copied().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::{Class, Obs, Status, core_of, cos_deg, exp_neg, median};

    #[test]
    fn deterministic_maths_matches_std() {
        for x in [0.0, -0.1, -0.5, -1.0, -4.5, -20.0, -100.0] {
            let (a, b) = (exp_neg(x), f64::exp(x));
            assert!((a - b).abs() <= 1e-12 * b, "{x}: {a} {b}");
        }
        assert_eq!(exp_neg(-1000.0), 0.0);
        for d in [0.0, 27.7, 45.0, 60.0, 89.9, 90.0, -30.0, 135.0, 200.0] {
            assert!((cos_deg(d) - d.to_radians().cos()).abs() < 1e-12, "{d}");
        }
    }

    #[test]
    fn median_resists_one_wild_point() {
        let o = |x, y, w| Obs { x, y, w };
        let m = median(&[
            o(0.0, 0.0, 1.0),
            o(10.0, 0.0, 1.0),
            o(0.0, 10.0, 1.0),
            o(5000.0, 5000.0, 1.0),
        ])
        .expect("median");
        assert!(m.0.hypot(m.1) < 20.0, "{m:?}");
        // Two points: the heavier wins; a tie keeps the first.
        assert_eq!(
            median(&[o(0.0, 0.0, 1.0), o(9.0, 9.0, 2.0)]),
            Some((9.0, 9.0))
        );
        assert_eq!(
            median(&[o(0.0, 0.0, 1.0), o(9.0, 9.0, 1.0)]),
            Some((0.0, 0.0))
        );
        // A dominant weight pins the median to its point (Vardi–Zhang).
        let m =
            median(&[o(0.0, 0.0, 10.0), o(100.0, 0.0, 1.0), o(0.0, 100.0, 1.0)]).expect("median");
        assert!(m.0.hypot(m.1) < 1e-6, "{m:?}");
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn the_core_climbs_to_the_village_not_the_seed() {
        // A village of 60 buildings 300 m east of the seed, a hamlet of 6 at the seed.
        let mut own: Vec<(f64, f64)> = Vec::new();
        for i in 0..60 {
            own.push((300.0 + f64::from(i % 8) * 10.0, f64::from(i / 8) * 10.0));
        }
        for i in 0..6 {
            own.push((f64::from(i) * 5.0, 0.0));
        }
        let (st, core, n) = core_of(&own, Class::Village);
        assert_eq!((st, n), (Status::Core, 66));
        let c = core.expect("core");
        assert!((c.0 - 335.0).abs() <= 50.0 && c.1.abs() <= 75.0, "{c:?}");
        // The same layout is the same bytes.
        assert_eq!(core_of(&own, Class::Village).1, core);
        // Too few buildings, none, and a cluster beyond the gate.
        assert_eq!(core_of(&own[..3], Class::Village).0, Status::Few);
        assert_eq!(core_of(&[], Class::Village).0, Status::NoBuildings);
        let far: Vec<(f64, f64)> = (0..40)
            .map(|i| (1_300.0 + f64::from(i), 0.0))
            .chain([(0.0, 0.0); 0])
            .collect();
        let st = core_of(&far, Class::Village).0;
        assert!(matches!(st, Status::NoDensity | Status::Far), "{st:?}");
    }
}
