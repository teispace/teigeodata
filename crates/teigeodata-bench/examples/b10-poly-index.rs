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
use teigeodata_core::table::Table;
use teigeodata_core::{E7, Point, hilbert};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::poly_index::{self, PolyIndex, Rect, polygon_box};
use teigeodata_format::polygons::Polygons;
use teigeodata_format::region::{self, Options, Region};
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
    // `B10_OVERVIEW=1`: only the overview A/B.
    if std::env::var_os("B10_OVERVIEW").is_some() {
        let rings: Vec<usize> = (0..n)
            .flat_map(|p| polys.parts(p).unwrap())
            .flat_map(|part| polys.rings(part).unwrap())
            .collect();
        overview(&pack, &arcs, &polys, &rings);
        return;
    }
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

    // The admin REGION with its own vertices (D-083's layout), every level in one section.
    let regions: Vec<Vec<Vec<[i32; 2]>>> = (0..n)
        .map(|p| {
            polys
                .all_rings(p)
                .map(|r| {
                    polys
                        .ring(&arcs, r, 0)
                        .map(|v| [v.lon(), v.lat()])
                        .collect()
                })
                .collect()
        })
        .collect();
    let t0 = Instant::now();
    let body = region::encode(&regions, &Options::default()).unwrap();
    let built = t0.elapsed();
    let file = pack_with(
        "places",
        1,
        &[(Kind::REGION, Encoding::TABLE, n as u64, &body)],
        false,
    )
    .unwrap();
    let rp = Pack::open(&file).unwrap();
    let reg = Region::open(&rp.section(Kind::REGION).unwrap()).unwrap();
    reg.check().unwrap();
    let table = Table::parse(&body).unwrap();
    let col = |id: u16| table.column_raw(id).map_or(0, |(_, b)| b.len());
    println!(
        "\nAdmin REGION, own vertices: {} B in {built:.1?}; {} leaves, {} runs, {} vertices; X+Y {} B, runs {} B",
        body.len(),
        reg.leaves(),
        reg.runs(),
        reg.vertices(),
        col(region::X) + col(region::Y),
        col(region::RUN_EDGE) + col(region::RUN_LEN) + col(region::RUN_ZONES)
    );
    let mut out = [0_u32; 8];
    let mut agree = 0;
    for p in &points {
        let k = reg.regions_at(*p, &mut out);
        let mut want: Vec<u32> = (0..n)
            .filter(|&q| polys.contains(&arcs, q, *p))
            .map(|q| q as u32)
            .collect();
        want.sort_unstable();
        if out.get(..k.min(8)) == Some(&want[..]) {
            agree += 1;
        }
    }
    let ns = time(points.len(), |i| {
        black_box(reg.regions_at(points[i], &mut [0_u32; 8]));
    });
    println!(
        "{agree} of {} uniform points agree with Polygons::contains; {ns:.0} ns per query (all levels)",
        points.len()
    );

    // The same REGION on the pack's ARCS: runs store their first vertex, the rest are deltas.
    let arc_pts: Vec<Vec<[i32; 2]>> = (0..arcs.len())
        .map(|a| {
            arcs.points(a, false, 0)
                .map(|v| [v.lon(), v.lat()])
                .collect()
        })
        .collect();
    let scales: Vec<u32> = if arcs.has_scale() {
        (0..arcs.len()).map(|a| arcs.scale(a)).collect()
    } else {
        Vec::new()
    };
    let uses: Vec<Vec<u32>> = (0..n)
        .map(|p| {
            polys
                .all_rings(p)
                .flat_map(|r| polys.refs(r).map(|(a, _)| a as u32))
                .collect()
        })
        .collect();
    let t0 = Instant::now();
    let shared = region::encode_on_arcs(&arc_pts, &scales, &uses, &Options::default()).unwrap();
    let built = t0.elapsed();
    let file2 = pack_with(
        "places",
        1,
        &[(Kind::REGION, Encoding::TABLE, n as u64, &shared)],
        false,
    )
    .unwrap();
    let rp2 = Pack::open(&file2).unwrap();
    let reg2 = Region::open_on_arcs(&rp2.section(Kind::REGION).unwrap(), &arcs).unwrap();
    let t1 = Instant::now();
    reg2.check().unwrap();
    let checked = t1.elapsed();
    let table2 = Table::parse(&shared).unwrap();
    let col2 = |id: u16| table2.column_raw(id).map_or(0, |(_, b)| b.len());
    println!(
        "Admin REGION on ARCS: {} B in {built:.1?} (check {checked:.1?}); {} leaves, {} runs; run first vertices {} B; top grid (DIR + TOP_ZONES) {} B; leaves (KEY, LEVEL, ZONES, NUDGE, RUNS) {} B",
        shared.len(),
        reg2.leaves(),
        reg2.runs(),
        col2(region::RUN_X) + col2(region::RUN_Y) + col2(region::RUN_SCALE),
        col2(region::DIR) + col2(region::TOP_ZONES),
        [
            region::KEY,
            region::LEVEL,
            region::ZONES,
            region::NUDGE,
            region::RUNS
        ]
        .iter()
        .map(|&c| col2(c))
        .sum::<usize>()
    );
    let mut same = 0;
    let (mut a1, mut a2) = ([0_u32; 8], [0_u32; 8]);
    for p in &points {
        let (k1, k2) = (reg.regions_at(*p, &mut a1), reg2.regions_at(*p, &mut a2));
        if a1[..k1.min(8)] == a2[..k2.min(8)] {
            same += 1;
        }
    }
    let ns2 = time(points.len(), |i| {
        black_box(reg2.regions_at(points[i], &mut [0_u32; 8]));
    });
    let ns1 = time(points.len(), |i| {
        black_box(reg.regions_at(points[i], &mut [0_u32; 8]));
    });
    println!(
        "{same} of {} points give the same regions; own vertices {ns1:.0} ns, on ARCS {ns2:.0} ns per query",
        points.len()
    );

    // Leaf size and top grid on ARCS, on uniform points and on the pack's place points.
    let places =
        teigeodata_format::places::Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
    let mut at_places: Vec<Point<E7>> = (0..places.len())
        .filter_map(|r| places.point(r))
        .map(|p| Point::<E7>::from_units(p.lat() * 10, p.lon() * 10).unwrap())
        .collect();
    // A fixed shuffle so the cache sees places in no spatial order.
    let mut srng = Rng::new(12);
    for i in (1..at_places.len()).rev() {
        at_places.swap(i, (srng.next_u64() % (i as u64 + 1)) as usize);
    }
    at_places.truncate(20_000);
    println!(
        "\n| Leaf max | Top | Fine cells | Fine grid | Bytes | Leaves | Runs | Uniform ns | Places ns | Check ms |"
    );
    println!("|---:|---:|---:|---|---:|---:|---:|---:|---:|---:|");
    let mut base: Option<Vec<[u32; 8]>> = None;
    for (leaf_max, top, fine_cells) in [
        (32, 10, 0),
        (32, 12, 0),
        (32, 8, 0),
        (32, 8, 1 << 14),
        (32, 8, 1 << 16),
        (32, 8, 1 << 18),
        (32, 8, 1 << 20),
        (16, 8, 1 << 16),
        (16, 8, 1 << 18),
        (64, 8, 1 << 18),
    ] {
        let opts = Options {
            leaf_max,
            top,
            fine_cells,
        };
        let body = region::encode_on_arcs(&arc_pts, &scales, &uses, &opts).unwrap();
        let file = pack_with(
            "places",
            1,
            &[(Kind::REGION, Encoding::TABLE, n as u64, &body)],
            false,
        )
        .unwrap();
        let rp = Pack::open(&file).unwrap();
        let r = Region::open_on_arcs(&rp.section(Kind::REGION).unwrap(), &arcs).unwrap();
        let t = Instant::now();
        r.check().unwrap();
        let check_ms = t.elapsed().as_secs_f64() * 1e3;
        // Every configuration answers the same.
        let answers: Vec<[u32; 8]> = points
            .iter()
            .chain(&at_places)
            .map(|&p| {
                let mut out = [u32::MAX; 8];
                r.regions_at(p, &mut out);
                out
            })
            .collect();
        match &base {
            None => base = Some(answers),
            Some(b) => assert!(*b == answers, "{opts:?} answers differently"),
        }
        let u = time(points.len(), |i| {
            black_box(r.regions_at(points[i], &mut [0_u32; 8]));
        });
        let pl = time(at_places.len(), |i| {
            black_box(r.regions_at(at_places[i], &mut [0_u32; 8]));
        });
        let grid = r
            .fine_grid()
            .map_or("none".to_string(), |(l, c, w)| format!("{l}: {c}×{w}"));
        println!(
            "| {leaf_max} | {top} | {fine_cells} | {grid} | {} | {} | {} | {u:.0} | {pl:.0} | {check_ms:.0} |",
            body.len(),
            r.leaves(),
            r.runs()
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
    overview(&pack, &arcs, &polys, &rings);
}

/// An overview (`arcs::overview`, D-102): a second ARCS of the same arcs holding only the vertices kept at level `k` and
/// coarser, so a coarse read decodes only those. Its bytes, and ring reads at each level from
/// it against the full ARCS (the same points, asserted).
fn overview(pack: &Pack<'_>, arcs: &Arcs<'_>, polys: &Polygons<'_>, rings: &[usize]) {
    let full = pack.section(Kind::ARCS).unwrap().bytes().len();
    println!("\nFull ARCS: {full} B for {} vertices.", arcs.vertices());
    println!(
        "\n| Overview from level | Vertices | Bytes | % of ARCS | ns per vertex read at its level | at level 6 |"
    );
    println!("|---:|---:|---:|---:|---:|---:|");
    for k in 1..=5_u8 {
        let body = teigeodata_format::arcs::overview(arcs, k).unwrap();
        let ov = Arcs::open(&teigeodata_format::Section::unpacked(
            Kind::ARCS_OVERVIEW,
            arcs.len() as u64,
            &body,
        ))
        .unwrap();
        ov.check().unwrap();
        ov.check_overview(arcs).unwrap();
        let per = |src: &Arcs<'_>, lod: u8| {
            let mut total = 0_usize;
            for &r in rings {
                total += polys.ring(src, r, lod).count();
            }
            time(1, |_| {
                let mut s = 0_i64;
                for &r in rings {
                    for v in polys.ring(src, r, lod) {
                        s += i64::from(v.lat());
                    }
                }
                black_box(s);
            }) / total as f64
        };
        for lod in k..=6 {
            for &r in rings {
                assert!(
                    polys.ring(arcs, r, lod).eq(polys.ring(&ov, r, lod)),
                    "ring {r} at level {lod} differs"
                );
            }
        }
        let arc_ns = time(1, |_| {
            let mut s = 0_i64;
            for a in 0..ov.len() {
                for v in ov.points(a, false, 6) {
                    s += i64::from(v.lat());
                }
            }
            black_box(s);
        }) / ov.len() as f64;
        let new_ns = time(1, |_| {
            for a in 0..ov.len() {
                black_box(ov.points(a, false, 6));
            }
        }) / ov.len() as f64;
        println!(
            "overview {k}: {arc_ns:.0} ns per arc read at level 6, {new_ns:.0} ns per iterator made"
        );
        println!(
            "| {k} | {} | {} | {:.1}% | {:.2} | {:.2} |",
            ov.vertices(),
            body.len(),
            body.len() as f64 * 100.0 / full as f64,
            per(&ov, k),
            per(&ov, 6)
        );
    }
}
