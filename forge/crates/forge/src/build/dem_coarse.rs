//! The coarse DEM (D-225, TG-10): a small elevation grid for phones, its own component
//! `dem-coarse`, beside the pack's 1″ DEM.
//!
//! - **Built from the pack's own DEM** (already EGM2008 and masked to the pack's area, D-192):
//!   `dem::coarsen` block-means each coarse cell from the 1″ samples in it, quantises it and
//!   codes it with the same coder, streaming one coarse tile at a time.
//! - **The grid:** [`DEFAULT`], chosen by measurement on Nepal (D-225): 15″ cells, an 8 m
//!   height step, 32-sample tiles.
//! - **G-dem-coarse** ([`gate`]): at every place with a height, the coarse height against the
//!   1″ DEM's. The error a coarse grid must make is the terrain's spread inside a cell: the
//!   predicted error at a place is the standard deviation σ of the 1″ samples of the coarse
//!   cell around it. The gate fails when the 90th percentile of |coarse − 1″| is above
//!   [`P90_RATIO`] × the 90th percentile of σ plus half the height step, or any place with a
//!   1″ height has none from the coarse grid. It also reads each of those cells at its coarse
//!   sample and fails when one is more than half a step (and 0.25 m of interpolation) from the
//!   mean of its 1″ samples, summed apart from the builder: an offset, a shift or a wrong
//!   factor the percentiles would miss (places sit on ridges and valley floors, so a bias moves
//!   P90 little: +16 m moved it by −2.6 m on Nepal).
//!
//! `forge dem-coarse <pack.tgd> [--arcsec N] [--step-m M] [--log2 L] [--plant-m H] [--out
//! <file>]` builds and gates a coarse grid from a written pack, for measurement; `--plant-m`
//! raises every coarse height by `H` metres before the gate (a planted fault it must catch);
//! `--out` writes the pack again with the coarse grid.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::time::Instant;

use teigeodata_core::{E7, PlaceKind, Point};
use teigeodata_format::dem::{
    self, Coarse, Dem, DemScratch, Family, Grid, THRESHOLDS, TileOptions,
};
use teigeodata_format::places::Places;
use teigeodata_format::{Encoding, Kind, Pack, Section, Tag, Writer};

/// The coarse grid of every pack with a DEM (D-225): 15″ of a 1″ DEM, heights in 8 m steps
/// (±4 m, far below the within-cell spread: Nepal's percentiles equal the 2 m step's to 0.2 m
/// for 54% fewer bytes), tiles of 32 × 32 samples (8′ a side: half the overreach of 64 for
/// 1% more bytes).
pub(crate) const DEFAULT: Coarse = Coarse {
    factor: 15,
    step_mm: 8_000,
    tile_log2: 5,
};

/// G-dem-coarse's bound: P90 of |coarse − 1″| at most this many times P90 of the predicted
/// error (the within-cell σ), plus half the height step. Measured on Nepal (D-225): 0.85 at
/// 15″, 0.93 at 30″.
pub(crate) const P90_RATIO: f64 = 1.0;

/// The `DEM_COARSE` section of the pack DEM `fine` (a DEM section body of `tiles` tiles).
pub(crate) fn build(fine: &[u8], tiles: u64, c: Coarse) -> Result<Option<(Vec<u8>, u64)>, String> {
    let dem = Dem::open(&Section::unpacked(Kind::DEM, tiles, fine)).map_err(|e| e.to_string())?;
    dem::coarsen(
        &dem,
        c,
        &Family::standard(),
        &THRESHOLDS,
        &TileOptions::default(),
    )
    .map_err(|e| format!("DEM_COARSE: {e}"))
}

/// G-dem-coarse's numbers.
#[derive(Debug)]
pub(crate) struct Measured {
    /// Places with a 1″ height.
    pub(crate) places: usize,
    /// Of those, with no coarse height.
    pub(crate) missing: usize,
    /// P50, P90, P99 and the largest |coarse − 1″|, metres.
    pub(crate) error: [f64; 4],
    /// P50, P90 and P99 of the predicted error, the within-cell σ, metres.
    pub(crate) predicted: [f64; 3],
    /// Half the coarse height step, metres.
    pub(crate) half_step: f64,
    /// The places' cells, each read at its coarse sample against the mean of its 1″ samples.
    pub(crate) cells: usize,
    /// Of those, read more than half a step (and 0.25 m of interpolation) from their mean.
    pub(crate) off_cells: usize,
    /// The largest |coarse sample − cell mean|, metres.
    pub(crate) worst_cell: f64,
}

impl Measured {
    /// The problems: a place without a coarse height, or P90 above its bound.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.missing > 0 {
            out.push(format!(
                "{} places with a 1″ height have none from the coarse grid",
                self.missing
            ));
        }
        if self.off_cells > 0 {
            out.push(format!(
                "{} of {} cells read more than half a step from the mean of their 1″ samples (worst {:.1} m)",
                self.off_cells, self.cells, self.worst_cell
            ));
        }
        let bound = P90_RATIO * self.predicted[1] + self.half_step;
        if self.error[1] > bound {
            out.push(format!(
                "P90 |coarse − 1″| {:.1} m is above {P90_RATIO} × the predicted P90 {:.1} m + {:.1} m = {bound:.1} m",
                self.error[1], self.predicted[1], self.half_step
            ));
        }
        out
    }

    /// The summary line.
    pub(crate) fn line(&self) -> String {
        format!(
            "G-dem-coarse (D-225): at {} places, |coarse − 1″| P50 {:.1} m, P90 {:.1} m, P99 {:.1} m, max {:.1} m; predicted (the 1″ samples' σ in the place's cell) P50 {:.1} m, P90 {:.1} m, P99 {:.1} m; bound P90 ≤ {P90_RATIO} × {:.1} + {:.1} m; the {} places' cells read within {:.2} m of their 1″ means.",
            self.places,
            self.error[0],
            self.error[1],
            self.error[2],
            self.error[3],
            self.predicted[0],
            self.predicted[1],
            self.predicted[2],
            self.predicted[1],
            self.half_step,
            self.cells,
            self.worst_cell,
        )
    }
}

/// A coarse cell of a fine grid: the fine grid's first tile (its key in the section), the
/// cell's row and column.
type Cell = (u32, u64, u64);

/// Running sums of a cell's 1″ samples: count, sum and sum of squares, in metres.
#[derive(Clone, Copy, Default)]
struct Spread {
    n: f64,
    sum: f64,
    sq: f64,
}

impl Spread {
    fn sigma(&self) -> f64 {
        if self.n < 1.0 {
            return 0.0;
        }
        let mean = self.sum / self.n;
        (self.sq / self.n - mean * mean).max(0.0).sqrt()
    }
}

/// The coarse cell of `fine` holding the point, at `factor`: the cell of its nearest sample.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sample rows and columns of a grid on the globe, far below 2^52"
)]
fn cell_of(at: Point<E7>, fine: &Grid, factor: u64) -> Cell {
    let r = ((900_000_000.0 - f64::from(at.lat())) * 36.0 / 10_000.0 / f64::from(fine.lat_step))
        .round() as u64;
    let c = ((1_800_000_000.0 + f64::from(at.lon())) * 36.0 / 10_000.0 / f64::from(fine.lon_step))
        .round() as u64;
    let h = factor / 2;
    (fine.first_tile, (r + h) / factor, (c + h) / factor)
}

/// G-dem-coarse: the coarse grid `coarse` against the 1″ DEM `fine` at `points` (each place
/// with a height). One pass decodes only the 1″ tiles under the places' cells.
pub(crate) fn gate(
    coarse: &Dem<'_>,
    fine: &Dem<'_>,
    points: impl Iterator<Item = Point<E7>>,
) -> Result<Measured, String> {
    let (mut sf, mut sc) = (Box::new(DemScratch::new()), Box::new(DemScratch::new()));
    let mut rows: Vec<(f64, Cell)> = Vec::new();
    let mut factors: HashMap<u32, u64> = HashMap::new();
    let mut fine_grids: HashMap<u32, Grid> = HashMap::new();
    let mut missing = 0;
    let mut half_step: f64 = 0.0;
    for at in points {
        let Some((hf, gf)) = fine.height(at, &mut sf) else {
            continue;
        };
        let Some((hc, gc)) = coarse.height(at, &mut sc) else {
            missing += 1;
            continue;
        };
        half_step = half_step.max(f64::from(gc.step_mm) / 2_000.0);
        let factor = u64::from(gc.lat_step / gf.lat_step.max(1)).max(1);
        if *factors.entry(gf.first_tile).or_insert(factor) != factor {
            return Err("a 1″ grid answered by coarse grids of two factors".into());
        }
        fine_grids.insert(gf.first_tile, gf);
        rows.push(((hc - hf).abs(), cell_of(at, &gf, factor)));
    }
    let wanted: HashSet<Cell> = rows.iter().map(|r| r.1).collect();
    let spread = spreads(fine, &factors, &wanted);
    // Each cell's coarse sample is the mean of its 1″ samples, within half a step.
    let (mut off_cells, mut worst_cell) = (0, 0.0_f64);
    for (&(key, row, col), sp) in &spread {
        let (Some(&factor), Some(g)) = (factors.get(&key), fine_grids.get(&key)) else {
            continue;
        };
        if sp.n < 1.0 {
            continue;
        }
        let at = sample_point(
            row * factor * u64::from(g.lat_step),
            col * factor * u64::from(g.lon_step),
        )
        .ok_or("a coarse sample off the globe")?;
        let read = coarse.elevation(at, &mut sc).unwrap_or(f64::NAN);
        let d = (read - sp.sum / sp.n).abs();
        if d.is_nan() || d > half_step + 0.25 {
            off_cells += 1;
        }
        worst_cell = if d.is_nan() {
            f64::INFINITY
        } else {
            worst_cell.max(d)
        };
    }
    let mut err: Vec<f64> = rows.iter().map(|r| r.0).collect();
    let mut sigma: Vec<f64> = rows
        .iter()
        .map(|r| spread.get(&r.1).map_or(0.0, Spread::sigma))
        .collect();
    err.sort_by(f64::total_cmp);
    sigma.sort_by(f64::total_cmp);
    let q = |v: &[f64], p: f64| super::elevation::quantile(v, p);
    Ok(Measured {
        places: rows.len() + missing,
        missing,
        error: [
            q(&err, 0.5),
            q(&err, 0.9),
            q(&err, 0.99),
            err.last().copied().unwrap_or(0.0),
        ],
        predicted: [q(&sigma, 0.5), q(&sigma, 0.9), q(&sigma, 0.99)],
        half_step,
        cells: spread.len(),
        off_cells,
        worst_cell,
    })
}

/// The point of the sample `rows` and `cols` (in 0.1″ from 90° N and 180° W) on the 1e-7° grid.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "0.1″ offsets inside the globe, below 2^24"
)]
fn sample_point(rows: u64, cols: u64) -> Option<Point<E7>> {
    let e7 = |v: u64| (v as f64 * 1e7 / 36_000.0).round() as i64;
    let lat = i32::try_from(900_000_000 - e7(rows)).ok()?;
    let lon = i32::try_from(e7(cols) - 1_800_000_000).ok()?;
    Point::from_units(lat, lon)
}

/// The spread of the 1″ samples of each cell in `wanted`, decoding each 1″ tile under them
/// once.
fn spreads(
    fine: &Dem<'_>,
    factors: &HashMap<u32, u64>,
    wanted: &HashSet<Cell>,
) -> HashMap<Cell, Spread> {
    let mut out: HashMap<Cell, Spread> = wanted.iter().map(|&c| (c, Spread::default())).collect();
    let mut scratch = Box::new(DemScratch::new());
    for g in 0..fine.grids() {
        let Some(grid) = fine.grid(g) else { continue };
        let Some(&factor) = factors.get(&grid.first_tile) else {
            continue;
        };
        let (log2, half) = (grid.tile_log2, factor / 2);
        // The grid's tiles under the wanted cells: cell R covers rows [R·f − h, R·f − h + f).
        let mut tiles: HashSet<(u64, u64)> = HashSet::new();
        for &(key, row, col) in wanted {
            if key != grid.first_tile {
                continue;
            }
            let r0 = (row * factor).saturating_sub(half);
            let c0 = (col * factor).saturating_sub(half);
            for tr in (r0 >> log2)..=((r0 + factor - 1) >> log2) {
                for tc in (c0 >> log2)..=((c0 + factor - 1) >> log2) {
                    tiles.insert((tr, tc));
                }
            }
        }
        let step = f64::from(grid.step_mm) / 1000.0;
        let side = 1_u64 << log2;
        for (tr, tc) in tiles {
            let (Some(dr), Some(dc)) = (
                tr.checked_sub(u64::from(grid.tile_row0)),
                tc.checked_sub(u64::from(grid.tile_col0)),
            ) else {
                continue;
            };
            if dr >= u64::from(grid.tile_rows) || dc >= u64::from(grid.tile_cols) {
                continue;
            }
            let tile = u64::from(grid.first_tile) + dr * u64::from(grid.tile_cols) + dc;
            let Some(samples) = usize::try_from(tile)
                .ok()
                .and_then(|t| fine.decode_tile(t, &mut scratch))
            else {
                continue;
            };
            for (i, &v) in (0_u64..).zip(samples.iter()) {
                let sr = (tr << log2) + i / side;
                let sc = (tc << log2) + i % side;
                let cell = (grid.first_tile, (sr + half) / factor, (sc + half) / factor);
                if let Some(acc) = out.get_mut(&cell) {
                    let metres = f64::from(v) * step;
                    acc.n += 1.0;
                    acc.sum += metres;
                    acc.sq += metres * metres;
                }
            }
        }
    }
    out
}

/// G-dem-coarse on a written pack: its `DEM_COARSE` checked and gated against its DEM at
/// every place with a height but timezone areas. The summary line, or the problems.
pub(crate) fn verify(
    coarse: &Section<'_>,
    fine: &Section<'_>,
    places: &Places<'_>,
) -> Result<String, String> {
    let c = Dem::open(coarse).map_err(|e| e.to_string())?;
    c.check().map_err(|e| format!("DEM_COARSE: {e}"))?;
    let f = Dem::open(fine).map_err(|e| e.to_string())?;
    let points = (0..places.len())
        .filter(|&r| places.kind(r) != Some(PlaceKind::TimezoneArea))
        .filter_map(|r| places.point(r))
        .map(Point::to_e7);
    let m = gate(&c, &f, points)?;
    let problems = m.problems();
    if !problems.is_empty() {
        return Err(format!("G-dem-coarse: {}", problems.join("; ")));
    }
    Ok(format!(
        "{} DEM_COARSE: {} tiles, {} B.",
        m.line(),
        c.tiles(),
        coarse.bytes().len()
    ))
}

/// Every sample of a coarse section raised by `metres` (in whole steps): G-dem-coarse's
/// planted fault.
pub(crate) fn raised(body: &[u8], tiles: u64, metres: i32) -> Result<Vec<u8>, String> {
    let fail = |e: teigeodata_format::Error| e.to_string();
    let coarse = Dem::open(&Section::unpacked(Kind::DEM_COARSE, tiles, body)).map_err(fail)?;
    let family = Family::standard();
    let opts = TileOptions::default();
    let mut scratch = Box::new(DemScratch::new());
    let mut grids: Vec<(Grid, Vec<Option<Vec<u8>>>)> = Vec::new();
    for g in 0..coarse.grids() {
        let Some(grid) = coarse.grid(g) else { continue };
        let by = metres * 1000 / i32::try_from(grid.step_mm).map_err(|e| e.to_string())?;
        let n = grid.tile_rows as usize * grid.tile_cols as usize;
        let mut codes = Vec::with_capacity(n);
        for t in 0..n {
            let q: Option<Vec<i32>> = coarse
                .decode_tile(grid.first_tile as usize + t, &mut scratch)
                .map(|q| q.iter().map(|v| v + by).collect());
            let code = q
                .map(|q| dem::encode_tile(&q, grid.tile_log2, &family, &THRESHOLDS, &opts))
                .transpose()
                .map_err(fail)?;
            codes.push(code);
        }
        grids.push((grid, codes));
    }
    let refs: Vec<Vec<Option<&[u8]>>> = grids
        .iter()
        .map(|(_, c)| c.iter().map(Option::as_deref).collect())
        .collect();
    let inputs: Vec<dem::GridInput<'_>> = grids
        .iter()
        .zip(&refs)
        .map(|((grid, _), tiles)| dem::GridInput { grid: *grid, tiles })
        .collect();
    dem::encode(&inputs, &family, &THRESHOLDS).map_err(fail)
}

/// `forge dem-coarse <pack.tgd> [--arcsec N] [--step-m M] [--log2 L] [--plant-m H] [--out
/// <file>]`.
pub(crate) fn run(args: &[String]) -> Result<String, String> {
    let usage = "usage: forge dem-coarse <pack.tgd> [--arcsec N] [--step-m M] [--log2 L] [--plant-m H] [--out <file>]";
    let mut it = args.iter();
    let path = it.next().ok_or(usage)?;
    let (mut c, mut out, mut plant) = (DEFAULT, None, 0);
    while let Some(flag) = it.next() {
        let v = it.next().ok_or(usage)?;
        let n = || v.parse::<u32>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--arcsec" => c.factor = n()?,
            "--step-m" => c.step_mm = n()? * 1000,
            "--log2" => c.tile_log2 = n()?,
            "--plant-m" => plant = v.parse::<i32>().map_err(|e| format!("{flag}: {e}"))?,
            "--out" => out = Some(v.clone()),
            _ => return Err(usage.into()),
        }
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let pack = Pack::open(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let fine = pack.section(Kind::DEM).ok_or("the pack has no DEM")?;
    let source_step = Dem::open(&fine)
        .map_err(|e| e.to_string())?
        .grid(0)
        .map_or(10, |g| g.lat_step);
    // `--arcsec` names the coarse spacing; the factor is it over the source's.
    c.factor = (c.factor * 10).checked_div(source_step).unwrap_or(0);
    let t = Instant::now();
    let (mut body, tiles) = build(fine.bytes(), fine.item_count(), c)?.ok_or("no coarse tiles")?;
    let built = t.elapsed();
    if plant != 0 {
        body = raised(&body, tiles, plant)?;
    }
    let zstd = zstd::bulk::compress(&body, 19)
        .map_err(|e| e.to_string())?
        .len();
    let section = Section::unpacked(Kind::DEM_COARSE, tiles, &body);
    let places = pack.section(Kind::PLACES).ok_or("the pack has no PLACES")?;
    let places = Places::open(&places).map_err(|e| e.to_string())?;
    let line = verify(&section, &fine, &places)?;
    let mut report = format!(
        "DEM_COARSE of {path} at {}″ ({} × the source), {} m steps, tiles of {}: {} B, {zstd} B at zstd 19, {tiles} tiles, built in {:.2} s.\n{line}\n",
        c.factor * source_step / 10,
        c.factor,
        c.step_mm / 1000,
        1 << c.tile_log2,
        body.len(),
        built.as_secs_f64(),
    );
    if let Some(to) = out {
        let written = with_section(&pack, (Kind::DEM_COARSE, tiles, &body))?;
        std::fs::write(&to, &written).map_err(|e| format!("{to}: {e}"))?;
        let _ = writeln!(report, "wrote {to}: {} B", written.len());
    }
    Ok(report)
}

/// `pack` written again with section `kind` added or replaced, its manifest kept.
fn with_section(
    pack: &Pack<'_>,
    (kind, items, body): (Kind, u64, &[u8]),
) -> Result<Vec<u8>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let m = pack.manifest();
    let mut w = Writer::new(m.address(), m.component(), m.release()).map_err(e)?;
    for (tag, value) in m.entries() {
        if ![Tag::ADDRESS, Tag::COMPONENT, Tag::RELEASE].contains(&tag) {
            w.manifest(tag, value).map_err(e)?;
        }
    }
    let mut sections: Vec<(Kind, Encoding, u64, &[u8])> = pack
        .sections()
        .filter(|s| s.kind() != kind && s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    sections.push((kind, Encoding::TABLE, items, body));
    sections.sort_by_key(|s| s.0);
    w.pack(&sections, None).map_err(e)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::many_single_char_names,
        reason = "test code"
    )]

    use teigeodata_core::{E7, Point};
    use teigeodata_format::dem::{
        self, Coarse, DATUM_EGM2008, Dem, Family, Grid, GridInput, THRESHOLDS, TileOptions,
    };
    use teigeodata_format::{Kind, Section};

    /// A 1″ grid of 2 × 2 tiles of 64 at row 974, column 4,078 (Nepal-like), heights in 1 m
    /// steps: a slope with ridges of `relief` metres every 12 samples.
    fn fine(relief: f64) -> (Vec<u8>, u64) {
        let family = Family::standard();
        let mut codes = Vec::new();
        for t in 0..4_i64 {
            let q: Vec<i32> = (0..4096_i64)
                .map(|i| {
                    let (r, c) = ((t / 2) * 64 + i / 64, (t % 2) * 64 + i % 64);
                    let h = 1500.0
                        + 2.0 * r as f64
                        + relief * (c as f64 / 12.0 * std::f64::consts::TAU).sin();
                    h.round() as i32
                })
                .collect();
            codes.push(
                dem::encode_tile(&q, 6, &family, &THRESHOLDS, &TileOptions::default()).unwrap(),
            );
        }
        let tiles: Vec<Option<&[u8]>> = codes.iter().map(|c| Some(c.as_slice())).collect();
        let grid = Grid {
            tile_row0: 974,
            tile_col0: 4_078,
            tile_rows: 2,
            tile_cols: 2,
            lat_step: 10,
            lon_step: 10,
            step_mm: 1_000,
            datum: DATUM_EGM2008,
            source: dem::Source::Nasadem.code(),
            tile_log2: 6,
            first_tile: 0,
        };
        let body = dem::encode(
            &[GridInput {
                grid,
                tiles: &tiles,
            }],
            &family,
            &THRESHOLDS,
        )
        .unwrap();
        (body, 4)
    }

    /// Points inside the grid, away from its edge.
    fn points() -> Vec<Point<E7>> {
        let mut out = Vec::new();
        for i in 0..400_i32 {
            let (r, c) = (
                974.0 * 64.0 + 10.0 + f64::from(i % 20) * 5.3,
                4_078.0 * 64.0 + 10.0 + f64::from(i / 20) * 5.1,
            );
            let (lat, lon) = (90.0 - r / 3600.0, -180.0 + c / 3600.0);
            out.push(Point::from_units((lat * 1e7) as i32, (lon * 1e7) as i32).unwrap());
        }
        out
    }

    const C: Coarse = Coarse {
        factor: 4,
        step_mm: 2_000,
        tile_log2: 4,
    };

    #[test]
    fn the_gate_passes_a_true_coarse_grid_and_fails_a_shifted_one() {
        let (body, n) = fine(30.0);
        let f = Dem::open(&Section::unpacked(Kind::DEM, n, &body)).unwrap();
        let (cb, cn) = super::build(&body, n, C).unwrap().unwrap();
        let c = Dem::open(&Section::unpacked(Kind::DEM_COARSE, cn, &cb)).unwrap();
        let m = super::gate(&c, &f, points().into_iter()).unwrap();
        assert_eq!(m.places, 400);
        assert_eq!(m.missing, 0);
        assert!(m.predicted[1] > 5.0, "{m:?}");
        assert_eq!(m.problems(), Vec::<String>::new(), "{}", m.line());
        assert_eq!(c.grid(0).unwrap().source, dem::Source::Nasadem.code());
        // Planted: every coarse height 26 m (13 steps) high.
        let bad = super::raised(&cb, cn, 26).unwrap();
        let b = Dem::open(&Section::unpacked(Kind::DEM_COARSE, cn, &bad)).unwrap();
        let m = super::gate(&b, &f, points().into_iter()).unwrap();
        assert!(m.off_cells > 0 && !m.problems().is_empty(), "{}", m.line());
        // One step (2 m) is caught by the cells alone.
        let bad = super::raised(&cb, cn, 2).unwrap();
        let b = Dem::open(&Section::unpacked(Kind::DEM_COARSE, cn, &bad)).unwrap();
        let m = super::gate(&b, &f, points().into_iter()).unwrap();
        assert!(m.off_cells > 0 && m.problems().len() == 1, "{}", m.line());
    }

    #[test]
    fn a_place_without_a_coarse_height_fails_the_gate() {
        let (body, n) = fine(0.0);
        let f = Dem::open(&Section::unpacked(Kind::DEM, n, &body)).unwrap();
        let (cb, cn) = super::build(&body, n, C).unwrap().unwrap();
        let c = Dem::open(&Section::unpacked(Kind::DEM_COARSE, cn, &cb)).unwrap();
        // Flat across each row: a slope only, so the error is the bilinear fit's, near 0.
        let m = super::gate(&c, &f, points().into_iter()).unwrap();
        assert!(m.error[1] <= 1.5, "{}", m.line());
        assert_eq!(m.problems(), Vec::<String>::new());
        // Planted: only the first coarse tile kept.
        let (masked, k) = dem::mask(&c, |g, r, col| r == g.tile_row0 && col == g.tile_col0)
            .unwrap()
            .unwrap();
        let m2 = Dem::open(&Section::unpacked(Kind::DEM_COARSE, k, &masked)).unwrap();
        let m = super::gate(&m2, &f, points().into_iter()).unwrap();
        assert!(m.missing > 0 && !m.problems().is_empty(), "{}", m.line());
    }
}
