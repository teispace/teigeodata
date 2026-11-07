//! B8: `PLACE_INDEX`, the packed Hilbert R-tree over the PLACES rows (D-081), against `rstar`
//! 0.14 (a bulk-loaded R*-tree in memory) and `geo-index` 0.4 (flatbush, f64 boxes, zero-copy).
//! Sizes per place, box queries at city (0.05°) and district (1°) scale, and the k = 1 and
//! k = 10 nearest places, on the `GeoNames` Nepal places (if present) and uniform world points.
//!
//! The rivals rank by planar distance in degrees, which is cheaper and wrong near the poles and
//! the antimeridian; ours ranks by great-circle distance. Run in release:
//! `cargo run --release -p teigeodata-bench --example b8-place-index`.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "a report printed for the results page; sizes far below 2^52; box corners a to d"
)]

use std::hint::black_box;
use std::ops::ControlFlow;
use std::time::Instant;

use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTreeBuilder, RTreeIndex};
use teigeodata_bench::{Rng, geonames_path, pack_with};
use teigeodata_core::{E6, PlaceKind, Point, hilbert};
use teigeodata_format::place_index::{self, Neighbour, PlaceIndex, Rect};
use teigeodata_format::places::{self, PlaceColumns, Places};
use teigeodata_format::{Encoding, Kind, Pack};

const DEG: i32 = 1_000_000;

/// Median ns per call of `f` over `n` calls, in 5 runs.
fn time<F: FnMut(usize)>(n: usize, mut f: F) -> f64 {
    let mut runs: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            for i in 0..n {
                f(i);
            }
            t.elapsed().as_nanos() as f64 / n as f64
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    runs[2]
}

fn nepal() -> Option<Vec<Point<E6>>> {
    let text = std::fs::read_to_string(geonames_path()).ok()?;
    let mut v: Vec<Point<E6>> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Point::from_degrees(f.get(4)?.parse().ok()?, f.get(5)?.parse().ok()?).ok()
        })
        .collect();
    v.sort_by_key(|p| (hilbert(*p), *p));
    Some(v)
}

fn world(n: usize, seed: u64) -> Vec<Point<E6>> {
    let mut r = Rng::new(seed);
    let mut v: Vec<Point<E6>> = (0..n)
        .filter_map(|_| {
            let a = r.next_u64();
            Point::from_units(
                (a % 180_000_001) as i32 - 90 * DEG,
                ((a >> 28) % 360_000_001) as i32 - 180 * DEG,
            )
        })
        .collect();
    v.sort_by_key(|p| (hilbert(*p), *p));
    v
}

/// Query points: near the data (a place moved up to 0.05°), as a user's position would be.
fn queries(pts: &[Point<E6>], n: usize, seed: u64) -> Vec<Point<E6>> {
    let mut r = Rng::new(seed);
    (0..n)
        .map(|_| {
            let p = pts[(r.next_u64() % pts.len() as u64) as usize];
            let j = |r: &mut Rng| (r.next_u64() % 100_001) as i32 - 50_000;
            Point::from_units(
                (p.lat() + j(&mut r)).clamp(-90 * DEG, 90 * DEG),
                (p.lon() + j(&mut r)).clamp(-180 * DEG, 180 * DEG),
            )
            .unwrap()
        })
        .collect()
}

fn run(name: &str, pts: &[Point<E6>]) {
    let n = pts.len();
    let kinds = vec![PlaceKind::Settlement; n];
    let places_body = places::encode(&PlaceColumns {
        points: pts,
        kinds: &kinds,
        ..PlaceColumns::default()
    })
    .unwrap();
    let qs = queries(pts, 20_000, 5);
    let boxes = |w: i32| -> Vec<Rect> {
        qs.iter()
            .map(|q| Rect {
                min_lat: (q.lat() - w / 2).max(-90 * DEG),
                max_lat: (q.lat() + w / 2).min(90 * DEG),
                min_lon: (q.lon() - w / 2).max(-180 * DEG),
                max_lon: (q.lon() + w / 2).min(180 * DEG),
            })
            .collect()
    };
    let (city, district) = (boxes(DEG / 20), boxes(DEG));
    let mean_hits = |b: &[Rect]| -> f64 {
        let inside = |q: &Rect, p: &Point<E6>| {
            q.min_lat <= p.lat()
                && p.lat() <= q.max_lat
                && q.min_lon <= p.lon()
                && p.lon() <= q.max_lon
        };
        b.iter()
            .take(200)
            .map(|q| pts.iter().filter(|p| inside(q, p)).count())
            .sum::<usize>() as f64
            / 200.0
    };
    println!(
        "\n**{name}**: {n} places; a city box holds {:.1} places on average, a district box {:.0}.\n",
        mean_hits(&city),
        mean_hits(&district)
    );
    println!("| Index | B per place | city box ns | district box ns | k = 1 ns | k = 10 ns |");
    println!("|---|---:|---:|---:|---:|---:|");
    for node in [8, 16, 32] {
        let body = place_index::encode(pts, node).unwrap();
        let sections = [
            (Kind::PLACES, Encoding::TABLE, n as u64, &places_body[..]),
            (Kind::PLACE_INDEX, Encoding::TABLE, n as u64, &body[..]),
        ];
        let file = pack_with("places", 1, &sections, false).unwrap();
        let pack = Pack::open(&file).unwrap();
        let pl = Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
        let ix = PlaceIndex::open(&pack.section(Kind::PLACE_INDEX).unwrap(), pl).unwrap();
        let count = |b: &[Rect]| {
            time(b.len(), |i| {
                let mut c = 0_usize;
                let _ = ix.within(black_box(b[i]), |_| {
                    c += 1;
                    ControlFlow::Continue(())
                });
                black_box(c);
            })
        };
        let (tc, td) = (count(&city), count(&district));
        let knn = |k: usize| {
            let mut out = vec![Neighbour::default(); k];
            time(qs.len(), |i| {
                black_box(ix.nearest(black_box(qs[i]), &mut out));
            })
        };
        println!(
            "| ours, node {node} | {:.2} | {tc:.0} | {td:.0} | {:.0} | {:.0} |",
            body.len() as f64 / n as f64,
            knn(1),
            knn(10)
        );
    }

    // geo-index: flatbush layout, f64 boxes, Hilbert sort, node 16.
    let mut b = RTreeBuilder::<f64>::new(n as u32);
    for p in pts {
        let (x, y) = (p.lon_degrees(), p.lat_degrees());
        b.add(x, y, x, y);
    }
    let gi = b.finish::<HilbertSort>();
    let deg = |r: &Rect| {
        (
            f64::from(r.min_lon) / 1e6,
            f64::from(r.min_lat) / 1e6,
            f64::from(r.max_lon) / 1e6,
            f64::from(r.max_lat) / 1e6,
        )
    };
    let gbox = |bs: &[Rect]| {
        time(bs.len(), |i| {
            let (a, b, c, d) = deg(&bs[i]);
            black_box(gi.search(a, b, c, d).len());
        })
    };
    let gknn = |k: usize| {
        time(qs.len(), |i| {
            black_box(gi.neighbors(qs[i].lon_degrees(), qs[i].lat_degrees(), Some(k), None));
        })
    };
    println!(
        "| `geo-index` 0.4, f64 | {:.2} | {:.0} | {:.0} | {:.0} | {:.0} |",
        gi.as_ref().len() as f64 / n as f64,
        gbox(&city),
        gbox(&district),
        gknn(1),
        gknn(10)
    );

    // rstar: an R*-tree in memory (no serialised form).
    let rt = rstar::RTree::bulk_load(
        pts.iter()
            .map(|p| [p.lon_degrees(), p.lat_degrees()])
            .collect(),
    );
    let rbox = |bs: &[Rect]| {
        time(bs.len(), |i| {
            let (a, b, c, d) = deg(&bs[i]);
            let env = rstar::AABB::from_corners([a, b], [c, d]);
            black_box(rt.locate_in_envelope(env).count());
        })
    };
    let rknn = |k: usize| {
        time(qs.len(), |i| {
            let q = [qs[i].lon_degrees(), qs[i].lat_degrees()];
            black_box(rt.nearest_neighbor_iter(q).take(k).count());
        })
    };
    println!(
        "| `rstar` 0.14 (in memory) | {:.1} (heap, est.) | {:.0} | {:.0} | {:.0} | {:.0} |",
        16.0 * 1.5,
        rbox(&city),
        rbox(&district),
        rknn(1),
        rknn(10)
    );
}

fn main() {
    println!("B8 PLACE_INDEX (median of 5 runs of 20,000 queries; single thread)");
    if let Some(np) = nepal() {
        run("Nepal (GeoNames)", &np);
    }
    run("World, uniform", &world(1_000_000, 1));
    run("World, uniform", &world(10_000_000, 2));
}
