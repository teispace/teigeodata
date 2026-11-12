//! Gate G-outline-parts (D-249): a layer that reads a pack's outline part of level `k`
//! (`LayerOptions::outline_level`) answers `admin_at` with exactly the units the whole pack
//! gives at that point, cut at level `k`: at every place of the pack and on a grid over the
//! places' box at `step` degrees, for every outline part the pack has. Any difference fails, and
//! the runner exits non-zero.
//!
//! `shift` compares part `k` with the whole chain cut at `k + shift` instead: any shift other
//! than 0 must fail (the planted fault).
//!
//! Usage: `cargo run --release -p teigeodata-engine --example admin-cut --
//! <pack.tgd> [step_deg=0.01] [shift=0]`
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a gate over local inputs: it reports on stdout"
)]

use std::process::ExitCode;

use teigeodata_core::{E7, PlaceId, PlaceKind, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Place, Scratch};
use teigeodata_format::places::Places;
use teigeodata_format::{Kind, Pack};

/// The pack at `path` alone in an engine, reading the outlines of `level` (`None`: all).
fn engine(path: &str, level: Option<u8>) -> Result<Engine, String> {
    let options = LayerOptions {
        outline_level: level,
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

/// Every place's point of the pack, a grid at `step` over their box, and the outline levels.
fn inputs(path: &str, step: f64) -> Result<(Vec<Point<E7>>, Vec<u8>), String> {
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
    let levels: Vec<u8> = pack
        .parts(Kind::POLYGONS)
        .filter_map(|s| s.part().outline_levels().map(|(d, _)| d))
        .collect();
    Ok((out, levels))
}

fn run(path: &str, step: f64, shift: u8) -> Result<usize, String> {
    let (points, levels) = inputs(path, step)?;
    if levels.is_empty() {
        return Err(format!("{path} has no outline parts"));
    }
    let whole = engine(path, None)?;
    let (mut s, mut out) = (Scratch::default(), Vec::new());
    let mut failed = 0;
    for &k in &levels {
        let cut = engine(path, Some(k))?;
        let (mut compared, mut held) = (0_usize, 0_usize);
        let mut first = None;
        for &at in &points {
            let want = units(&whole, at, k.saturating_add(shift), &mut s, &mut out)?;
            let got = units(&cut, at, u8::MAX, &mut s, &mut out)?;
            compared += 1;
            held += usize::from(!want.is_empty());
            if want != got {
                failed += 1;
                first.get_or_insert((at, want, got));
            }
        }
        println!("level {k}: {compared} points, {held} inside a unit");
        if let Some((at, want, got)) = first {
            println!(
                "FAIL {:.6},{:.6}: the whole pack cut at {} gives {want:?}, the part {got:?}",
                at.lat_degrees(),
                at.lon_degrees(),
                k.saturating_add(shift)
            );
        }
    }
    Ok(failed)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: admin-cut <pack.tgd> [step_deg=0.01] [shift=0]");
        return ExitCode::from(2);
    };
    let step = args.get(1).map_or(Ok(0.01), |v| v.parse::<f64>());
    let shift = args.get(2).map_or(Ok(0), |v| v.parse::<u8>());
    let (Ok(step), Ok(shift)) = (step, shift) else {
        eprintln!("step_deg is a number above 0, shift a small integer");
        return ExitCode::from(2);
    };
    if step.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        eprintln!("step_deg must be above 0");
        return ExitCode::from(2);
    }
    match run(path, step, shift) {
        Ok(0) => {
            println!("G-outline-parts: ok");
            ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("G-outline-parts: FAIL, {n} points differ");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
