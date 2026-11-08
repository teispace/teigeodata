//! The `dem-section` stage (B11, D-089): a DEM tileset's heights over a box of whole degrees,
//! quantised and coded as the pack's DEM section by `teigeodata_format::dem`.
//!
//! ```text
//! forge dem-section <tileset> <west,south,east,north> [--step-mm 2000]
//! ```
//!
//! - **Grid:** 1″ samples (`PixelIsPoint`; source tiles of 3600 × 3600 whose first row and
//!   column lie on whole degrees), cut into 64 × 64 tiles on the global grid from 90° N, 180° W.
//!   A source tile on any other grid stops the stage (Copernicus above 50° N widens its
//!   longitude spacing; that needs a second grid and is not built yet).
//! - **Coverage:** the source tiles inside the box. A DEM tile with no covered sample is absent;
//!   in a partly covered one (the box's edge, or the sea, where Copernicus has no tile) each
//!   uncovered sample takes the nearest covered sample of its row, or of the nearest row with
//!   one, so no false cliff costs bits. A void inside a source tile stops the stage.
//! - **Bounded memory:** DEM tiles are made in strips of [`STRIP`] source columns, band by band
//!   (64 rows) from north to south; only the source tiles the current band needs stay decoded
//!   (at most 2 rows × (`STRIP` + 1) columns, as `i32` steps), so memory does not grow with the
//!   box. Source tiles decode and DEM tiles encode in parallel.
//! - **Checks:** every tile is read back by the writer; the section is then checked whole
//!   (`Dem::check`), and the heights at 20,000 random points are compared with bilinear
//!   sampling of the source: they must agree within half a step.
//!
//! Output: `data/build/dem-section/<tileset>/dem.tgd` (a pack of the DEM section alone, for B11)
//! and `summary.md`.

#![allow(
    clippy::many_single_char_names,
    reason = "a box's west, south, east, north; rows r, columns c; tiles t and paths p"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "the forge runs on 64-bit hosts: sample indices (u64) below 2^40 fit usize"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices come from the same arrays (tile samples, 3600 × 3600 source tiles): an out-of-range index is a forge bug and must stop the build"
)]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use teigeodata_core::{E7, Point};
use teigeodata_format::dem::{
    self, DATUM_EGM2008, Dem, DemScratch, Family, GridInput, THRESHOLDS, TileOptions,
};
use teigeodata_format::{Encoding, Kind, Pack, Section, Writer};

use super::Grid as Source;
use crate::lock::Lock;
use crate::tiles::{self, Tile};

/// Samples per source tile side (1° at 1″).
const SRC: u64 = 3600;
/// log2 of the DEM tile side.
const LOG2: u32 = 6;
const SIDE: u64 = 1 << LOG2;
/// Source columns per strip.
const STRIP: u64 = 3;
/// Source tiles decoded at once.
const PARALLEL: usize = 4;
/// Random points compared with the source after the build.
const PROBES: usize = 20_000;

/// A decoded source tile as quantised steps, or `None` where nothing covers it.
type Cell = Option<Arc<Vec<i32>>>;

/// The box in samples: rows `[r0, r1)` from 90° N, columns `[c0, c1)` from 180° W.
#[derive(Clone, Copy, Debug)]
struct Extent {
    r0: u64,
    r1: u64,
    c0: u64,
    c1: u64,
}

fn parse_box(text: &str) -> Result<(i32, i32, i32, i32), String> {
    let v: Vec<i32> = text
        .split(',')
        .map(|x| x.trim().parse::<i32>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("box `{text}`: {e}"))?;
    match v.as_slice() {
        &[w, s, e, n]
            if w <= e && s <= n && (-180..180).contains(&w) && e < 180 && s >= -90 && n < 90 =>
        {
            Ok((w, s, e, n))
        }
        _ => Err(format!(
            "box `{text}`: expected west,south,east,north in whole degrees (corners of 1° tiles)"
        )),
    }
}

/// `h / step` rounded to the nearest step, halves away from zero.
#[allow(
    clippy::cast_possible_truncation,
    reason = "heights in steps are far below 2^31"
)]
fn quantise(h: f64, step_mm: u32) -> i32 {
    let v = h * 1000.0 / f64::from(step_mm);
    v.round() as i32
}

/// Reads one source tile and quantises it, after checking its grid.
fn load(path: &Path, sr: u64, sc: u64, step_mm: u32) -> Result<Vec<i32>, String> {
    let g = Source::open(path)?;
    let name = path.display();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    #[allow(clippy::cast_precision_loss, reason = "whole degrees")]
    let (lat0, lon0) = (90.0 - sr as f64 / 3600.0, sc as f64 / 3600.0 - 180.0);
    if g.w != SRC as usize
        || g.h != SRC as usize
        || !close(g.dx * 3600.0, 1.0)
        || !close(g.dy * 3600.0, 1.0)
        || !close(g.lat0, lat0)
        || !close(g.lon0, lon0)
    {
        return Err(format!(
            "{name}: not a 1″ 3600 × 3600 tile at {lat0}°, {lon0}° (grid {} × {}, step {}″ × {}″, origin {}°, {}°)",
            g.w,
            g.h,
            g.dx * 3600.0,
            g.dy * 3600.0,
            g.lat0,
            g.lon0
        ));
    }
    g.z.iter()
        .map(|&h| {
            if h.is_finite() && h > -1000.0 {
                Ok(quantise(f64::from(h), step_mm))
            } else {
                Err(format!("{name}: a void ({h}); voids are not supported"))
            }
        })
        .collect()
}

/// One DEM tile's samples from the decoded source tiles, gaps filled; `None` if nothing covers
/// it.
fn gather(tr: u64, tc: u64, cache: &HashMap<(u64, u64), Cell>) -> Option<(Vec<i32>, usize)> {
    let n = SIDE as usize;
    let mut v: Vec<Option<i32>> = vec![None; n * n];
    let mut covered = 0;
    for y in 0..n {
        let r = tr * SIDE + y as u64;
        for x in 0..n {
            let c = tc * SIDE + x as u64;
            if let Some(Some(t)) = cache.get(&(r / SRC, c / SRC)) {
                v[y * n + x] = Some(t[((r % SRC) * SRC + c % SRC) as usize]);
                covered += 1;
            }
        }
    }
    if covered == 0 {
        return None;
    }
    // Fill each row from its nearest covered sample, then empty rows from the nearest full row.
    let mut row_ok = vec![false; n];
    for y in 0..n {
        let row = &mut v[y * n..(y + 1) * n];
        let Some(first) = row.iter().position(Option::is_some) else {
            continue;
        };
        row_ok[y] = true;
        let mut last = row[first];
        for (x, s) in row.iter_mut().enumerate() {
            if x < first {
                *s = last;
            } else if s.is_some() {
                last = *s;
            } else {
                *s = last;
            }
        }
    }
    for y in 0..n {
        if !row_ok[y] {
            let near = (0..n)
                .filter(|&k| row_ok[k])
                .min_by_key(|&k| (k.abs_diff(y), k))?;
            for x in 0..n {
                v[y * n + x] = v[near * n + x];
            }
        }
    }
    Some((v.into_iter().map(|s| s.unwrap_or(0)).collect(), covered))
}

/// What one DEM tile came to.
struct Made {
    index: usize,
    code: Option<Vec<u8>>,
    covered: usize,
}

/// `forge dem-section <tileset> <box> [--step-mm N] [--no-refine] [--thresholds a,b,c,d,e,f,g]`.
#[allow(clippy::too_many_lines, reason = "one stage, read top to bottom")]
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let usage = "usage: forge dem-section <tileset> <west,south,east,north> \
                 [--step-mm 2000] [--no-refine] [--thresholds a,b,c,d,e,f,g]";
    let (id, bx) = match args {
        [id, bx, ..] => (id.as_str(), bx.as_str()),
        _ => return Err(usage.into()),
    };
    let mut step_mm = 2_000u32;
    let mut opts = TileOptions::default();
    let mut thresholds = THRESHOLDS;
    let mut rest = args.get(2..).unwrap_or_default().iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--step-mm" => {
                let v = rest.next().ok_or(usage)?;
                step_mm = v.parse().map_err(|e| format!("--step-mm: {e}"))?;
            }
            "--no-refine" => opts.refine = false,
            "--thresholds" => {
                let v = rest.next().ok_or(usage)?;
                let parsed: Vec<u32> = v
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("--thresholds: {e}"))?;
                thresholds = parsed
                    .try_into()
                    .map_err(|_| "--thresholds takes seven values".to_string())?;
                if thresholds.windows(2).any(|w| w[0] >= w[1]) {
                    return Err("--thresholds must increase".into());
                }
            }
            _ => return Err(usage.into()),
        }
    }
    if step_mm == 0 {
        return Err("--step-mm must be positive".into());
    }
    let t0 = Instant::now();
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let source = lock.get(id)?;
    let template = source
        .query_params
        .get("tile")
        .ok_or_else(|| format!("{id}: no tile name template"))?;
    let manifest = tiles::manifest(root, source)?;
    let by_name: BTreeMap<&str, &Tile> = manifest.iter().map(|t| (t.name.as_str(), t)).collect();
    let (w, s, e, n) = parse_box(bx)?;
    let to_u = |v: i32| u64::try_from(v).map_err(|_| "box off the globe".to_string());
    let ext = Extent {
        r0: to_u(90 - n - 1)? * SRC,
        r1: to_u(90 - s)? * SRC,
        c0: to_u(w + 180)? * SRC,
        c1: to_u(e + 181)? * SRC,
    };
    // The source tile at global source row `sr` and column `sc`, if the box and the manifest
    // have it.
    let path_of = |sr: u64, sc: u64| -> Result<Option<std::path::PathBuf>, String> {
        if sr * SRC < ext.r0 || sr * SRC >= ext.r1 || sc * SRC < ext.c0 || sc * SRC >= ext.c1 {
            return Ok(None);
        }
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            reason = "degrees"
        )]
        let (lat, lon) = (89 - sr as i32, sc as i32 - 180);
        let name = tiles::tile_name(template, lon, lat);
        match by_name.get(name.as_str()) {
            None => Err(format!("{name}: not in the {id} manifest")),
            Some(t) if t.sha256.is_none() => Ok(None),
            Some(t) => match tiles::tile_path(root, t) {
                Some(p) if p.is_file() => Ok(Some(p)),
                _ => Err(format!("{name}: not fetched (run `forge fetch {id}`)")),
            },
        }
    };
    let (tr0, tr1) = (ext.r0 / SIDE, (ext.r1 - 1) / SIDE + 1);
    let (tc0, tc1) = (ext.c0 / SIDE, (ext.c1 - 1) / SIDE + 1);
    let (rows, cols) = (tr1 - tr0, tc1 - tc0);
    let family = Family::standard();
    let mut codes: Vec<Option<Vec<u8>>> =
        vec![None; usize::try_from(rows * cols).map_err(|e| e.to_string())?];
    let mut covered_total = 0_usize;
    // Bytes and samples of DEM tiles wholly inside one source tile, by that source tile.
    let mut per_source: BTreeMap<(u64, u64), (u64, u64)> = BTreeMap::new();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(PARALLEL)
        .build()
        .map_err(|e| e.to_string())?;
    let mut decoded = 0_usize;
    let (sc_first, sc_last) = (ext.c0 / SRC, (ext.c1 - 1) / SRC);
    let mut strip = sc_first;
    while strip <= sc_last {
        let strip_end = (strip + STRIP).min(sc_last + 1);
        // DEM tile columns whose first sample lies in this strip (the first strip also takes
        // any that start west of the box).
        let tcs: Vec<u64> = (tc0..tc1)
            .filter(|&tc| {
                let sc = (tc * SIDE) / SRC;
                (sc >= strip || strip == sc_first) && sc < strip_end
            })
            .collect();
        let mut cache: HashMap<(u64, u64), Cell> = HashMap::new();
        for tr in tr0..tr1 {
            let (sr_a, sr_b) = ((tr * SIDE) / SRC, (tr * SIDE + SIDE - 1) / SRC);
            cache.retain(|&(sr, _), _| sr >= sr_a);
            let need: Vec<(u64, u64)> = (sr_a..=sr_b)
                .flat_map(|sr| (strip..=strip_end.min(sc_last)).map(move |sc| (sr, sc)))
                .filter(|k| !cache.contains_key(k))
                .collect();
            let loaded: Vec<((u64, u64), Cell)> = pool.install(|| {
                need.par_iter()
                    .map(|&(sr, sc)| {
                        Ok((
                            (sr, sc),
                            match path_of(sr, sc)? {
                                Some(p) => Some(Arc::new(load(&p, sr * SRC, sc * SRC, step_mm)?)),
                                None => None,
                            },
                        ))
                    })
                    .collect::<Result<_, String>>()
            })?;
            decoded += loaded.iter().filter(|(_, c)| c.is_some()).count();
            cache.extend(loaded);
            let made: Vec<Made> = pool.install(|| {
                tcs.par_iter()
                    .map(|&tc| {
                        let index =
                            usize::try_from((tr - tr0) * cols + (tc - tc0)).unwrap_or(usize::MAX);
                        match gather(tr, tc, &cache) {
                            None => Ok(Made {
                                index,
                                code: None,
                                covered: 0,
                            }),
                            Some((q, covered)) => Ok(Made {
                                index,
                                code: Some(
                                    dem::encode_tile(&q, LOG2, &family, &thresholds, &opts)
                                        .map_err(|e| e.to_string())?,
                                ),
                                covered,
                            }),
                        }
                    })
                    .collect::<Result<_, String>>()
            })?;
            for m in made {
                covered_total += m.covered;
                let tc = tc0 + (m.index as u64) % cols;
                let (sr, sc) = ((tr * SIDE) / SRC, (tc * SIDE) / SRC);
                let whole = m.covered == (SIDE * SIDE) as usize
                    && (tr * SIDE + SIDE - 1) / SRC == sr
                    && (tc * SIDE + SIDE - 1) / SRC == sc;
                if let (true, Some(c)) = (whole, &m.code) {
                    let slot = per_source.entry((sr, sc)).or_default();
                    slot.0 += c.len() as u64;
                    slot.1 += SIDE * SIDE;
                }
                codes[m.index] = m.code;
            }
        }
        strip = strip_end;
    }
    let refs: Vec<Option<&[u8]>> = codes.iter().map(Option::as_deref).collect();
    let u32_of = |v: u64| u32::try_from(v).map_err(|e| e.to_string());
    let grid = dem::Grid {
        tile_row0: u32_of(tr0)?,
        tile_col0: u32_of(tc0)?,
        tile_rows: u32_of(rows)?,
        tile_cols: u32_of(cols)?,
        lat_step: 10,
        lon_step: 10,
        step_mm,
        datum: DATUM_EGM2008,
        tile_log2: LOG2,
        first_tile: 0,
    };
    let body = dem::encode(&[GridInput { grid, tiles: &refs }], &family, &thresholds)
        .map_err(|e| e.to_string())?;
    let built = t0.elapsed();
    let section = Section::unpacked(Kind::DEM, rows * cols, &body);
    let d = Dem::open(&section).map_err(|e| e.to_string())?;
    let t1 = Instant::now();
    d.check().map_err(|e| e.to_string())?;
    let checked = t1.elapsed();
    let worst = probe(root, &d, &ext, &path_of, step_mm)?;
    let dir = root.join("data/build/dem-section").join(id);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // A pack of this section alone, for B11; not a pinned source, it feeds no build.
    let mut w = Writer::new("zz", "dem", "0.0.0").map_err(|e| e.to_string())?;
    w.build_id(0).created_unix(0);
    let tile_count = rows * cols;
    let bytes = w
        .pack(&[(Kind::DEM, Encoding::TABLE, tile_count, &body)], None)
        .map_err(|e| e.to_string())?;
    let read = Pack::open(&bytes).map_err(|e| e.to_string())?;
    let back = read
        .section(Kind::DEM)
        .ok_or("the pack lost its DEM section")?;
    Dem::open(&back)
        .map_err(|e| e.to_string())?
        .check()
        .map_err(|e| e.to_string())?;
    fs::write(dir.join("dem.tgd"), &bytes).map_err(|e| e.to_string())?;
    let present = codes.iter().filter(|c| c.is_some()).count();
    let payload: usize = codes.iter().flatten().map(Vec::len).sum();
    let mut out = format!(
        "# dem-section: {id} over {bx}, step {step_mm} mm, thresholds {thresholds:?}{}\n\n",
        if opts.refine { "" } else { ", no refinement" }
    );
    #[allow(
        clippy::cast_precision_loss,
        reason = "counts and bytes far below 2^52"
    )]
    {
        let _ = writeln!(
            out,
            "{present} of {} tiles of {SIDE} × {SIDE} present ({} source tiles decoded); {covered_total} samples covered; section {} B, payload {payload} B: {:.3} bits per covered sample. Built in {:.1?}, checked in {checked:.1?}; {PROBES} random points within {worst:.4} m of the source (limit {} m).\n",
            rows * cols,
            decoded,
            body.len(),
            payload as f64 * 8.0 / covered_total.max(1) as f64,
            built,
            f64::from(step_mm) / 2000.0
        );
        let _ = writeln!(
            out,
            "| Source tile | Whole DEM tiles | Bits per sample |\n|---|---:|---:|"
        );
        for ((sr, sc), (bytes, samples)) in &per_source {
            #[allow(
                clippy::cast_possible_wrap,
                clippy::cast_possible_truncation,
                reason = "degrees"
            )]
            let name = tiles::tile_name(template, *sc as i32 - 180, 89 - *sr as i32);
            let _ = writeln!(
                out,
                "| {name} | {} | {:.3} |",
                samples / (SIDE * SIDE),
                *bytes as f64 * 8.0 / *samples as f64
            );
        }
    }
    fs::write(dir.join("summary.md"), &out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Heights at random points against bilinear sampling of the source tiles: the largest
/// difference, which must be at most half a step.
fn probe(
    _root: &Path,
    d: &Dem<'_>,
    ext: &Extent,
    path_of: &dyn Fn(u64, u64) -> Result<Option<std::path::PathBuf>, String>,
    step_mm: u32,
) -> Result<f64, String> {
    let mut x = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // Points by source tile, so each tile decodes once.
    let mut by_tile: BTreeMap<(u64, u64), Vec<(u64, u64)>> = BTreeMap::new();
    for _ in 0..PROBES {
        // In units of 1e-3 sample, inside the box but not on its last row or column.
        let r = ext.r0 * 1000 + next() % ((ext.r1 - ext.r0 - 1) * 1000);
        let c = ext.c0 * 1000 + next() % ((ext.c1 - ext.c0 - 1) * 1000);
        by_tile
            .entry((r / 1000 / SRC, c / 1000 / SRC))
            .or_default()
            .push((r, c));
    }
    let limit = f64::from(step_mm) / 2000.0;
    let mut worst = 0.0_f64;
    let mut s = Box::new(DemScratch::new());
    for ((sr, sc), pts) in by_tile {
        let Some(p) = path_of(sr, sc)? else { continue };
        let g = Source::open(&p)?;
        for (r, c) in pts {
            // The point on the 1e-7° grid, as the reader sees it.
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                reason = "degrees"
            )]
            let (lat, lon) = (
                ((90.0 - r as f64 / 3_600_000.0) * 1e7).round() as i32,
                ((c as f64 / 3_600_000.0 - 180.0) * 1e7).round() as i32,
            );
            let at = Point::<E7>::from_units(lat, lon).ok_or("a probe off the grid")?;
            // Skip points whose bilinear cell reaches into the next source tile.
            let (rr, cc) = (r / 1000 % SRC, c / 1000 % SRC);
            if rr + 1 >= SRC || cc + 1 >= SRC {
                continue;
            }
            let want = g
                .sample(at.lon_degrees(), at.lat_degrees())
                .map(|(h, _)| f64::from(h));
            let got = d.elevation(at, &mut s);
            match (want, got) {
                (Some(w), Some(v)) => worst = worst.max((w - v).abs()),
                _ => return Err(format!("{at}: source {want:?}, section {got:?}")),
            }
        }
    }
    // The source's bilinear is in f32: allow its rounding on top of half a step.
    if worst > limit + 1e-3 {
        return Err(format!(
            "the section is {worst} m from the source (limit {limit} m)"
        ));
    }
    Ok(worst)
}
