//! Gate G-tz-layer (D-245): an embedded layer's `tz_at` gives the zone its country pack gives,
//! wherever the pack answers. The layer carries no timezone REGION: it answers by the zone of
//! its deepest admin unit holding the point (D-151), on outlines simplified to the profile's
//! tolerance. The pack answers by its REGION at full resolution. A point where they differ
//! passes only when it lies within `tol_m` of a zone line, that is when the pack gives the
//! layer's zone, or no zone, at a point that far away in one of eight directions. Every other
//! difference fails, and the runner exits non-zero.
//!
//! The points are every place of the pack and a grid over the places' box at `step` degrees.
//!
//! Usage: `cargo run --release -p teigeodata-engine --example tz-agree --
//! <layer.tgd> <pack.tgd> [step_deg=0.01] [tol_m=20]`
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a gate over local inputs: it reports on stdout"
)]

use std::process::ExitCode;

use teigeodata_core::{E7, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions};
use teigeodata_format::places::Places;
use teigeodata_format::{Kind, Pack};

/// Metres per degree of latitude (mean Earth radius 6,371 km).
const M_PER_DEG: f64 = 111_194.93;

/// A layer alone in its own engine.
fn engine(path: &str) -> Result<Engine, String> {
    let layer = Layer::open(path, LayerOptions::default()).map_err(|e| format!("{path}: {e}"))?;
    let mut e = Engine::new();
    e.add(layer);
    Ok(e)
}

/// The tzdb name of the zone `e` gives at `at`.
fn zone(e: &Engine, at: Point<E7>) -> Result<Option<String>, String> {
    let Some(area) = e.tz_at(at).map_err(|x| x.to_string())? else {
        return Ok(None);
    };
    Ok(e.zone_name(&area)
        .map_err(|x| x.to_string())?
        .map(str::to_owned))
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

/// Every place's point of the pack at `path`, and the box around them.
fn places(path: &str) -> Result<(Vec<Point<E7>>, [f64; 4]), String> {
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
    Ok((out, bbox))
}

/// What the runner found.
#[derive(Default)]
struct Tally {
    points: usize,
    compared: usize,
    border: usize,
    failed: usize,
}

fn run(layer: &str, pack: &str, step: f64, tol_m: f64) -> Result<Tally, String> {
    let (lay, pk) = (engine(layer)?, engine(pack)?);
    let (mut points, [south, west, north, east]) = places(pack)?;
    // Grid lines by index, so steps do not add up rounding error.
    let mut i = 0_u32;
    while south + step * f64::from(i) <= north + step {
        let lat = south + step * f64::from(i);
        let mut j = 0_u32;
        while west + step * f64::from(j) <= east + step {
            if let Ok(pt) = Point::<E7>::from_degrees(lat, west + step * f64::from(j)) {
                points.push(pt);
            }
            j += 1;
        }
        i += 1;
    }
    let mut t = Tally {
        points: points.len(),
        ..Tally::default()
    };
    for at in points {
        let Some(want) = zone(&pk, at)? else { continue };
        t.compared += 1;
        let got = zone(&lay, at)?;
        if got.as_deref() == Some(want.as_str()) {
            continue;
        }
        let mut near_line = false;
        for q in around(at, tol_m) {
            let z = zone(&pk, q)?;
            if z.is_none() || z == got {
                near_line = true;
                break;
            }
        }
        if near_line {
            t.border += 1;
        } else {
            t.failed += 1;
            if t.failed <= 20 {
                println!(
                    "FAIL {:.6},{:.6}: the pack gives {want}, the layer {}",
                    at.lat_degrees(),
                    at.lon_degrees(),
                    got.as_deref().unwrap_or("none")
                );
            }
        }
    }
    Ok(t)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(layer), Some(pack)) = (args.first(), args.get(1)) else {
        eprintln!("usage: tz-agree <layer.tgd> <pack.tgd> [step_deg=0.01] [tol_m=20]");
        return ExitCode::from(2);
    };
    let num = |i: usize, d: f64| args.get(i).map_or(Ok(d), |v| v.parse::<f64>());
    let (Ok(step), Ok(tol_m)) = (num(2, 0.01), num(3, 20.0)) else {
        eprintln!("step_deg and tol_m are numbers");
        return ExitCode::from(2);
    };
    if !(step > 0.0 && tol_m >= 0.0) {
        eprintln!("step_deg must be above 0 and tol_m not below 0");
        return ExitCode::from(2);
    }
    match run(layer, pack, step, tol_m) {
        Ok(t) => {
            println!(
                "{} points, {} in the pack's zones; {} differ within {tol_m} m of a zone line, \
                 {} beyond",
                t.points, t.compared, t.border, t.failed
            );
            if t.compared == 0 {
                println!("G-tz-layer: the pack answers at no point");
                return ExitCode::FAILURE;
            }
            if t.failed > 0 {
                println!("G-tz-layer: FAIL");
                return ExitCode::FAILURE;
            }
            println!("G-tz-layer: ok");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
