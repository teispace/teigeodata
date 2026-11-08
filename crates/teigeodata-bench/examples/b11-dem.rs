//! B11: decode speed of the DEM section (D-089). Whole tiles (ns per sample), cold point
//! queries (every query in a new tile, so it starts the tile and decodes up to its row), and
//! warm ones (a path of points close together, so the scratch goes on from where it stopped).
//!
//! Input: the pack `B11_PACK` names, else `data/build/dem-section/copernicus-glo30-np/dem.tgd`
//! (`forge dem-section copernicus-glo30-np <box>`). Run in release:
//! `cargo run --release -p teigeodata-bench --example b11-dem`.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a report printed for the results page; inputs checked by the run"
)]

use std::hint::black_box;
use std::time::Instant;

use teigeodata_bench::Rng;
use teigeodata_core::{E7, Point};
use teigeodata_format::dem::{Dem, DemScratch};
use teigeodata_format::{Kind, Pack};

const PACK: &str = "data/build/dem-section/copernicus-glo30-np/dem.tgd";

/// The fastest of `runs` timings of `f`, in nanoseconds per `per` items: on a shared machine
/// preemption only adds time, so the minimum is the stable statistic.
fn min_ns(runs: usize, per: usize, mut f: impl FnMut()) -> f64 {
    (0..runs)
        .map(|_| {
            let t0 = Instant::now();
            f();
            t0.elapsed().as_nanos() as f64 / per as f64
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let path = std::env::var("B11_PACK").unwrap_or_else(|_| PACK.to_string());
    let bytes = std::fs::read(&path).expect("the DEM pack (see the module comment)");
    let pack = Pack::open(&bytes).unwrap();
    let section = pack.section(Kind::DEM).expect("a DEM section");
    let dem = Dem::open(&section).unwrap();
    let grid = dem.grid(0).unwrap();
    let side = grid.side();
    let samples = dem.tiles() * side * side;
    println!(
        "{path}: {} B section, {} tiles of {side} × {side}, step {} mm",
        section.bytes().len(),
        dem.tiles(),
        grid.step_mm
    );

    let mut s = DemScratch::new();
    let whole = min_ns(15, samples, || {
        for t in 0..dem.tiles() {
            black_box(dem.decode_tile(t, &mut s));
        }
    });
    println!(
        "whole tiles: {whole:.2} ns per sample, {:.1} µs per tile",
        whole * (side * side) as f64 / 1000.0
    );

    // The grid's extent in 1e-7°: 0.1″ = 1e-7° × 1e4 / 36.
    let units =
        |tiles: u32, step: u32| i64::from(tiles) * side as i64 * i64::from(step) * 10_000 / 36;
    let north = 900_000_000 - units(grid.tile_row0, grid.lat_step);
    let south = north - units(grid.tile_rows, grid.lat_step);
    let west = -1_800_000_000 + units(grid.tile_col0, grid.lon_step);
    let east = west + units(grid.tile_cols, grid.lon_step);
    let mut rng = Rng::new(11);
    let mut within = |lo: i64, hi: i64| lo + (rng.next_u64() % (hi - lo).unsigned_abs()) as i64;
    let n = 20_000_usize;
    let cold: Vec<Point<E7>> = (0..n)
        .map(|_| {
            Point::from_units(
                within(south + 1, north - 1) as i32,
                within(west, east - 1) as i32,
            )
            .unwrap()
        })
        .collect();
    // A path: steps of about 30 m (2,700 units) east, wrapping to the next row of the path.
    let (lat0, lon0) = (within(south + 1, north - 1), within(west, east - 1));
    let warm: Vec<Point<E7>> = (0..n as i64)
        .map(|k| {
            let lon = west + (lon0 - west + k * 2_700).rem_euclid(east - west);
            Point::from_units(lat0 as i32, lon as i32).unwrap()
        })
        .collect();
    for (name, points) in [("cold (random points)", &cold), ("warm (a path)", &warm)] {
        let mut found = 0;
        let ns = min_ns(15, points.len(), || {
            let mut s = DemScratch::new();
            found = 0;
            for &p in points {
                if black_box(dem.elevation(p, &mut s)).is_some() {
                    found += 1;
                }
            }
        });
        println!(
            "{name}: {:.2} µs per query, {found} of {} found",
            ns / 1000.0,
            points.len()
        );
    }
}
