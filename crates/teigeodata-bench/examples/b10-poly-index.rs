//! B10: `POLY_INDEX` and ring reads on the built Nepal pack (D-088). The polygon index's size and
//! the nodes a query reads, for three leaf orders at node sizes 8, 16 and 32:
//!
//! - **level-first, row order** (what POLYGONS stores: grouped by admin level, each level by
//!   PLACES row, which is Hilbert order of the place's point);
//! - **level-first, box order** (each level by the Hilbert key of its boxes' centres);
//! - **one Hilbert sort** of every box centre, levels mixed (a level filter then prunes
//!   nothing).
//!
//! Then point → admin unit through `PolyIndex::containing` per level, and the decode speed of
//! every ring at each level of detail.
//!
//! Input: `data/build/pack/np/np-places-main.tgd` (`forge make np`). Run in release:
//! `cargo run --release -p teigeodata-bench --example b10-poly-index`.
#![allow(
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a report printed for the results page; inputs checked by the run"
)]

use std::hint::black_box;
use std::ops::{ControlFlow, Range};
use std::time::Instant;

use teigeodata_bench::{Rng, pack_with};
use teigeodata_core::{E7, Point, hilbert};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::poly_index::{self, PolyIndex, Rect, polygon_box};
use teigeodata_format::polygons::Polygons;
use teigeodata_format::{Encoding, Kind, Pack};

const PACK: &str = "data/build/pack/np/np-places-main.tgd";

fn centre_key(b: &Rect) -> u64 {
    let p = Point::<E7>::from_units(
        i32::midpoint(b.min_lat, b.max_lat) * 10,
        i32::midpoint(b.min_lon, b.max_lon) * 10,
    )
    .unwrap();
    hilbert(p)
}

/// Median ns per call over `n` calls, in 5 runs.
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

fn main() {
    let Ok(bytes) = std::fs::read(PACK) else {
        println!("{PACK} is missing: run `forge make np --release 2026.11.0` first");
        return;
    };
    let pack = Pack::open(&bytes).unwrap();
    let arcs = Arcs::open(&pack.section(Kind::ARCS).unwrap()).unwrap();
    let polys = Polygons::open(&pack.section(Kind::POLYGONS).unwrap()).unwrap();
    let stored = PolyIndex::open(&pack.section(Kind::POLY_INDEX).unwrap(), &polys).unwrap();
    stored.check(&polys, &arcs).unwrap();
    let n = polys.len();
    let groups: Vec<Range<usize>> = (0..polys.groups())
        .map(|g| polys.group(g).unwrap())
        .collect();
    let boxes: Vec<Rect> = (0..n)
        .map(|p| polygon_box(&polys, &arcs, p).unwrap())
        .collect();
    let mut bounds = Rect::EMPTY;
    for b in &boxes {
        bounds.grow(b);
    }
    println!(
        "{n} polygons in {} levels {:?}; stored index: node {}, {} levels, {} B ({:.2} B per polygon)",
        groups.len(),
        groups
            .iter()
            .map(ExactSizeIterator::len)
            .collect::<Vec<_>>(),
        stored.node_size(),
        stored.depth(),
        pack.section(Kind::POLY_INDEX).unwrap().bytes().len(),
        pack.section(Kind::POLY_INDEX).unwrap().bytes().len() as f64 / n as f64
    );

    // Leaf orders: a permutation of the polygons for each, and the group ranges it keeps.
    let level_rows: Vec<usize> = (0..n).collect();
    let mut level_boxes = Vec::new();
    for g in &groups {
        let mut v: Vec<usize> = g.clone().collect();
        v.sort_by_key(|&p| centre_key(&boxes[p]));
        level_boxes.extend(v);
    }
    let mut mixed: Vec<usize> = (0..n).collect();
    mixed.sort_by_key(|&p| centre_key(&boxes[p]));
    let orders: [(&str, &[usize], bool); 3] = [
        ("level-first, row order", &level_rows, true),
        ("level-first, box order", &level_boxes, true),
        ("one Hilbert sort", &mixed, false),
    ];

    // Queries: points uniform over the country's box, and viewports of 0.05° and 1°.
    let mut rng = Rng::new(10);
    let mut uniform =
        |lo: i32, hi: i32| lo + (rng.next_u64() % u64::from((hi - lo).cast_unsigned() + 1)) as i32;
    let points: Vec<Point<E7>> = (0..20_000)
        .map(|_| {
            Point::<E7>::from_units(
                uniform(bounds.min_lat, bounds.max_lat) * 10,
                uniform(bounds.min_lon, bounds.max_lon) * 10,
            )
            .unwrap()
        })
        .collect();
    let views = |size: i32, rng: &mut Rng| -> Vec<Rect> {
        (0..2_000)
            .map(|_| {
                let lat = bounds.min_lat
                    + (rng.next_u64()
                        % u64::from((bounds.max_lat - bounds.min_lat).cast_unsigned()))
                        as i32;
                let lon = bounds.min_lon
                    + (rng.next_u64()
                        % u64::from((bounds.max_lon - bounds.min_lon).cast_unsigned()))
                        as i32;
                Rect {
                    min_lat: lat,
                    max_lat: lat + size,
                    min_lon: lon,
                    max_lon: lon + size,
                }
            })
            .collect()
    };
    let mut vrng = Rng::new(11);
    let city = views(50_000, &mut vrng);
    let district = views(1_000_000, &mut vrng);

    println!(
        "\n| Order | Node | Bytes | B/polygon | Nodes per point, by level {:?} | Nodes per point, all levels | 0.05° view | 1° view |",
        (0..groups.len()).collect::<Vec<_>>()
    );
    println!("|---|---:|---:|---:|---|---:|---:|---:|");
    for (name, order, grouped) in orders {
        for node in [8, 16, 32] {
            let ordered: Vec<Rect> = order.iter().map(|&p| boxes[p]).collect();
            let body = poly_index::encode(&ordered, node).unwrap();
            let file = pack_with(
                "places",
                1,
                &[(Kind::POLY_INDEX, Encoding::TABLE, n as u64, &body)],
                false,
            )
            .unwrap();
            let p = Pack::open(&file).unwrap();
            let ix = PolyIndex::open(&p.section(Kind::POLY_INDEX).unwrap(), &polys).unwrap();
            let mean = |qs: &[Rect], range: &Range<usize>| {
                qs.iter()
                    .map(|q| ix.nodes_read(*q, range.clone()))
                    .sum::<usize>() as f64
                    / qs.len() as f64
            };
            let cells: Vec<Rect> = points
                .iter()
                .map(|p| Rect::around_e7(p.lat(), p.lon()))
                .collect();
            let per_level: Vec<String> = groups
                .iter()
                .map(|g| {
                    let range = if grouped { g.clone() } else { 0..n };
                    format!("{:.1}", mean(&cells, &range))
                })
                .collect();
            println!(
                "| {name} | {node} | {} | {:.2} | {} | {:.1} | {:.1} | {:.1} |",
                body.len(),
                body.len() as f64 / n as f64,
                per_level.join(" / "),
                mean(&cells, &(0..n)),
                mean(&city, &(0..n)),
                mean(&district, &(0..n)),
            );
        }
    }

    // Point → admin unit per level through the stored index (exact, rings tested).
    println!("\n| Level | Polygons | Candidates per point | Found per point | ns per lookup |");
    println!("|---:|---:|---:|---:|---:|");
    for (k, g) in groups.iter().enumerate() {
        let (mut cand, mut found) = (0_usize, 0_usize);
        for p in &points {
            let cell = Rect::around_e7(p.lat(), p.lon());
            let _ = stored.intersecting(cell, g.clone(), |_| {
                cand += 1;
                ControlFlow::Continue(())
            });
            let _ = stored.containing(&polys, &arcs, *p, g.clone(), |_| {
                found += 1;
                ControlFlow::Continue(())
            });
        }
        let ns = time(points.len(), |i| {
            let mut hit = usize::MAX;
            let _ = stored.containing(&polys, &arcs, points[i], g.clone(), |q| {
                hit = q;
                ControlFlow::Break(())
            });
            black_box(hit);
        });
        println!(
            "| {k} | {} | {:.2} | {:.3} | {ns:.0} |",
            g.len(),
            cand as f64 / points.len() as f64,
            found as f64 / points.len() as f64
        );
    }

    // Ring decode speed: every ring of every polygon, at each level of detail.
    let rings: Vec<usize> = (0..n)
        .flat_map(|p| polys.parts(p).unwrap())
        .flat_map(|part| polys.rings(part).unwrap())
        .collect();
    println!("\n| Level of detail | Vertices read | ns per vertex |");
    println!("|---:|---:|---:|");
    for lod in 0..=6_u8 {
        let mut total = 0_usize;
        for &r in &rings {
            total += polys.ring(&arcs, r, lod).count();
        }
        let ns = time(1, |_| {
            let mut s = 0_i64;
            for &r in &rings {
                for v in polys.ring(&arcs, r, lod) {
                    s += i64::from(v.lat());
                }
            }
            black_box(s);
        }) / total as f64;
        println!("| {lod} | {total} | {ns:.2} |");
    }
}
