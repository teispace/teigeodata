//! Gate G-outline-parts (D-249, D-250): a layer that reads a pack's outline part of level `k`
//! (`LayerOptions::outline_level`) at level of detail `lod` (`outline_tolerance_m`) answers
//! `admin_at` with the units the whole pack gives at that point, cut at level `k`: at every
//! place of the pack and on a grid over the places' box at `step` degrees, for every outline
//! part the pack has. At full resolution any difference fails; at a coarser level a point may
//! differ only where the whole pack gives the part's answer within the level's tolerance of
//! it (a border moved by the simplification). The runner exits non-zero on a failure.
//!
//! `shift` compares part `k` with the whole chain cut at `k + shift` instead, and `slack`
//! scales the tolerance: any shift other than 0, or a slack of 0 on a pack with coarse parts,
//! must fail (the planted faults).
//!
//! Usage: `cargo run --release -p teigeodata-engine --example admin-cut --
//! <pack.tgd> [step_deg=0.01] [shift=0] [slack=1]`
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a gate over local inputs: it reports on stdout"
)]

use std::process::ExitCode;

use teigeodata_core::{E7, PlaceId, PlaceKind, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Place, Scratch};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::places::Places;
use teigeodata_format::{Kind, Pack};

/// Metres in a degree of latitude (the mean Earth radius).
const M_PER_DEG: f64 = 111_194.93;

/// The pack at `path` alone in an engine, reading the outlines of `level` (`None`: all) at the
/// coarsest level of detail within `tolerance_m`.
fn engine(path: &str, level: Option<u8>, tolerance_m: u32) -> Result<Engine, String> {
    let options = LayerOptions {
        outline_level: level,
        outline_tolerance_m: tolerance_m,
        ..LayerOptions::default()
    };
    let layer = Layer::open(path, options).map_err(|e| format!("{path}: {e}"))?;
    let mut e = Engine::new();
    e.add(layer);
    Ok(e)
}

/// The IDs of the units holding `at`, of level `k` and coarser.
fn units(
    e: &Engine,
    at: Point<E7>,
    k: u8,
    s: &mut Scratch,
    out: &mut Vec<Place>,
) -> Result<Vec<PlaceId>, String> {
    e.admin_at(at, s, out).map_err(|x| x.to_string())?;
    let mut ids: Vec<PlaceId> = out
        .iter()
        .filter(|p| p.kind == Some(PlaceKind::Country) || p.admin_level <= k)
        .map(|p| p.id)
        .collect();
    ids.sort_unstable();
    Ok(ids)
}

/// `at` moved `m` metres in each of eight directions.
fn around(at: Point<E7>, m: f64) -> impl Iterator<Item = Point<E7>> {
    let (lat, lon) = (at.lat_degrees(), at.lon_degrees());
    let dlat = m / M_PER_DEG;
    let dlon = dlat / lat.to_radians().cos().max(1e-6);
    (0..8_u8).filter_map(move |k| {
        let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
        Point::<E7>::from_degrees(lat + dlat * a.sin(), lon + dlon * a.cos()).ok()
    })
}

/// One outline part: its deepest level, its level of detail and that level's tolerance in
/// metres (0 at full resolution).
type Cut = (u8, u8, u32);

/// Every place's point of the pack, a grid at `step` over their box, and the outline parts.
fn inputs(path: &str, step: f64) -> Result<(Vec<Point<E7>>, Vec<Cut>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let pack = Pack::open(&bytes).map_err(|e| e.to_string())?;
    let s = pack.section(Kind::PLACES).ok_or("the pack has no PLACES")?;
    let p = Places::open(&s).map_err(|e| e.to_string())?;
    let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut out = Vec::with_capacity(p.len());
    for r in 0..p.len() {
        let Some(pt) = p.point(r) else { continue };
        let (lat, lon) = (pt.lat_degrees(), pt.lon_degrees());
        bbox = [
            bbox[0].min(lat),
            bbox[1].min(lon),
            bbox[2].max(lat),
            bbox[3].max(lon),
        ];
        out.push(pt.to_e7());
    }
    let [south, west, north, east] = bbox;
    // Grid lines by index, so steps do not add up rounding error.
    let mut i = 0_u32;
    while south + step * f64::from(i) <= north + step {
        let lat = south + step * f64::from(i);
        let mut j = 0_u32;
        while west + step * f64::from(j) <= east + step {
            if let Ok(pt) = Point::<E7>::from_degrees(lat, west + step * f64::from(j)) {
                out.push(pt);
            }
            j += 1;
        }
        i += 1;
    }
    Ok((out, cuts(&pack)?))
}

/// The pack's outline parts, each with its tolerance from the base ARCS.
fn cuts(pack: &Pack<'_>) -> Result<Vec<Cut>, String> {
    let arcs = pack
        .section(Kind::ARCS)
        .map(|s| Arcs::open(&s))
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for s in pack.parts(Kind::POLYGONS) {
        let Some((deepest, lod)) = s.part().outline_levels() else {
            continue;
        };
        let tolerance_m = match lod {
            0 => 0,
            l => {
                arcs.as_ref()
                    .and_then(|a| a.tolerance_cm(l))
                    .ok_or("a coarse part without its tolerance")?
                    / 100
            }
        };
        out.push((deepest, lod, tolerance_m));
    }
    Ok(out)
}

/// What one part's comparison found.
#[derive(Default)]
struct Tally {
    compared: usize,
    held: usize,
    /// Different, with the part's answer within the tolerance.
    border: usize,
    failed: usize,
}

fn run(path: &str, step: f64, shift: u8, slack: f64) -> Result<usize, String> {
    let (points, cuts) = inputs(path, step)?;
    if cuts.is_empty() {
        return Err(format!("{path} has no outline parts"));
    }
    let whole = engine(path, None, 0)?;
    let (mut s, mut out) = (Scratch::default(), Vec::new());
    let mut failed = 0;
    for &(k, lod, tolerance_m) in &cuts {
        let cut = engine(path, Some(k), tolerance_m)?;
        let want_k = k.saturating_add(shift);
        let mut t = Tally::default();
        let mut first = None;
        for &at in &points {
            let want = units(&whole, at, want_k, &mut s, &mut out)?;
            let got = units(&cut, at, u8::MAX, &mut s, &mut out)?;
            t.compared += 1;
            t.held += usize::from(!want.is_empty());
            if want == got {
                continue;
            }
            let m = f64::from(tolerance_m) * slack;
            let mut near = false;
            if m > 0.0 {
                for q in around(at, m) {
                    if units(&whole, q, want_k, &mut s, &mut out)? == got {
                        near = true;
                        break;
                    }
                }
            }
            if near {
                t.border += 1;
            } else {
                t.failed += 1;
                first.get_or_insert((at, want, got));
            }
        }
        println!(
            "level {k} at lod {lod} ({tolerance_m} m): {} points, {} inside a unit, {} differ \
             within the tolerance, {} beyond",
            t.compared, t.held, t.border, t.failed
        );
        if let Some((at, want, got)) = first {
            println!(
                "FAIL {:.6},{:.6}: the whole pack cut at {want_k} gives {want:?}, the part {got:?}",
                at.lat_degrees(),
                at.lon_degrees(),
            );
        }
        failed += t.failed;
    }
    Ok(failed)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: admin-cut <pack.tgd> [step_deg=0.01] [shift=0] [slack=1]");
        return ExitCode::from(2);
    };
    let step = args.get(1).map_or(Ok(0.01), |v| v.parse::<f64>());
    let shift = args.get(2).map_or(Ok(0), |v| v.parse::<u8>());
    let slack = args.get(3).map_or(Ok(1.0), |v| v.parse::<f64>());
    let (Ok(step), Ok(shift), Ok(slack)) = (step, shift, slack) else {
        eprintln!("step_deg and slack are numbers, shift a small integer");
        return ExitCode::from(2);
    };
    if step.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater)
        || slack
            .partial_cmp(&0.0)
            .is_none_or(core::cmp::Ordering::is_lt)
    {
        eprintln!("step_deg must be above 0, slack at least 0");
        return ExitCode::from(2);
    }
    match run(path, step, shift, slack) {
        Ok(0) => {
            println!("G-outline-parts: ok");
            ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("G-outline-parts: FAIL, {n} points beyond the tolerance");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
