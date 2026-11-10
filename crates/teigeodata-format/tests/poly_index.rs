//! `POLY_INDEX` against brute force on deep trees: thousands of rectangles in three groups over
//! the whole world, including the antimeridian and the poles' neighbourhood, at node sizes 4 to
//! 32, in Hilbert and in random order.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "test code: model values are small, indices come from the same vectors, and w, s, e, n name edges"
)]

use std::ops::{ControlFlow, Range};

use teigeodata_core::bitpack::{Packed, encode_auto};
use teigeodata_core::table::{Table, encode as table};
use teigeodata_core::{E7, Point, hilbert};
use teigeodata_format::arcs::{self, ArcChunks, ArcColumns, Arcs};
use teigeodata_format::poly_index::{
    self, BOXES, NODE_SIZE, PolyIndex, QUANTA, ROOT, Rect, polygon_box,
};
use teigeodata_format::polygons::{self, PolygonInput, Polygons, arc_ref};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

const W: i32 = 1_800_000_000;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let span = u64::try_from(i64::from(hi) - i64::from(lo) + 1).unwrap();
        (i64::from(lo) + (self.next() % span) as i64) as i32
    }
}

/// A rectangle polygon on the 1e-7° grid: west, south, east, north.
type Model = [i32; 4];

/// Rectangles in three groups (coarse to fine), some on the antimeridian, each group ordered by
/// the Hilbert key of its centre or left in random order.
fn model(seed: u64, hilbert_order: bool) -> (Vec<Model>, [u32; 3]) {
    let mut r = Rng(seed);
    let sizes = [(150, 50_000_000), (700, 5_000_000), (2_500, 300_000)];
    let mut out = Vec::new();
    let mut groups = [0; 3];
    for (g, &(n, max)) in sizes.iter().enumerate() {
        groups[g] = out.len() as u32;
        let mut level: Vec<Model> = (0..n)
            .map(|k| {
                let (w, h) = (r.range(10, max), r.range(10, max));
                let south = r.range(-850_000_000, 850_000_000 - h);
                let west = match k % 23 {
                    0 => W - w,
                    1 => -W,
                    _ => r.range(-W, W - w),
                };
                [west, south, west + w, south + h]
            })
            .collect();
        if hilbert_order {
            level.sort_by_key(|m| {
                let c =
                    Point::<E7>::from_units(i32::midpoint(m[1], m[3]), i32::midpoint(m[0], m[2]))
                        .unwrap();
                hilbert(c)
            });
        }
        out.extend(level);
    }
    (out, groups)
}

fn at(lat: i32, lon: i32) -> Point<E7> {
    Point::<E7>::from_units(lat, lon).unwrap()
}

/// A pack with ARCS, POLYGONS and `POLY_INDEX` over the model; `index` replaces the written index.
fn pack(m: &[Model], groups: &[u32], node: usize, index: Option<&[u8]>) -> Vec<u8> {
    let rings: Vec<Vec<Point<E7>>> = m
        .iter()
        .map(|&[w, s, e, n]| vec![at(s, w), at(s, e), at(n, e), at(n, w), at(s, w)])
        .collect();
    let pts: Vec<&[Point<E7>]> = rings.iter().map(Vec::as_slice).collect();
    let arcs_body = arcs::encode(&ArcColumns {
        arcs: &pts,
        ..ArcColumns::default()
    })
    .unwrap();
    let refs: Vec<[u32; 1]> = (0..m.len() as u32).map(|a| [arc_ref(a, false)]).collect();
    let ring_lists: Vec<[&[u32]; 1]> = refs.iter().map(|r| [&r[..]]).collect();
    let parts: Vec<[&[&[u32]]; 1]> = ring_lists.iter().map(|r| [&r[..]]).collect();
    let input: Vec<PolygonInput<'_>> = m
        .iter()
        .zip(&parts)
        .enumerate()
        .map(|(i, (&[w, s, e, n], parts))| PolygonInput {
            row: i as u32,
            parts: &parts[..],
            label: at(i32::midpoint(s, n), i32::midpoint(w, e)),
            area_ha: 1,
        })
        .collect();
    let poly_body = polygons::encode(&input, groups).unwrap();
    let boxes: Vec<Rect> = m
        .iter()
        .map(|&[w, s, e, n]| {
            let mut b = Rect::around_e7(s, w);
            b.grow(&Rect::around_e7(n, e));
            b
        })
        .collect();
    let written = poly_index::encode(&boxes, node).unwrap();
    let ix_body = index.unwrap_or(&written);
    let mut wr = Writer::new("np", "places", "2026.11.0").unwrap();
    let n = m.len() as u64;
    wr.section_bytes(Kind::ARCS, Encoding::TABLE, n, &arcs_body)
        .unwrap();
    wr.section_bytes(Kind::POLYGONS, Encoding::TABLE, n, &poly_body)
        .unwrap();
    wr.section_bytes(Kind::POLY_INDEX, Encoding::TABLE, n, ix_body)
        .unwrap();
    wr.layout(false)
        .unwrap()
        .assemble(&[&arcs_body, &poly_body, ix_body])
        .unwrap()
}

struct Open<'a> {
    a: Arcs<'a>,
    v: Polygons<'a>,
    ix: Result<PolyIndex<'a>, Error>,
}

fn open<'a>(p: &Pack<'a>) -> Open<'a> {
    let a = Arcs::open(&p.section(Kind::ARCS).unwrap()).unwrap();
    let v = Polygons::open(&p.section(Kind::POLYGONS).unwrap()).unwrap();
    let ix = PolyIndex::open(&p.section(Kind::POLY_INDEX).unwrap(), &v);
    Open { a, v, ix }
}

/// Whether box `b` meets query `q`, which crosses the antimeridian when `min_lon > max_lon`.
fn meets(b: &Rect, q: &Rect) -> bool {
    let lat = b.min_lat <= q.max_lat && q.min_lat <= b.max_lat;
    let lon = if q.min_lon <= q.max_lon {
        b.min_lon <= q.max_lon && q.min_lon <= b.max_lon
    } else {
        b.max_lon >= q.min_lon || b.min_lon <= q.max_lon
    };
    lat && lon
}

fn grow_by(b: &Rect, d: i32) -> Rect {
    Rect {
        min_lat: b.min_lat - d,
        max_lat: b.max_lat + d,
        min_lon: b.min_lon - d,
        max_lon: b.max_lon + d,
    }
}

fn queries(r: &mut Rng) -> Vec<Rect> {
    let mut v = vec![
        Rect {
            min_lat: -90_000_000,
            max_lat: 90_000_000,
            min_lon: -180_000_000,
            max_lon: 180_000_000,
        },
        Rect {
            min_lat: -10_000_000,
            max_lat: 10_000_000,
            min_lon: 179_000_000,
            max_lon: -179_000_000,
        },
        Rect {
            min_lat: 1,
            max_lat: 0,
            min_lon: 0,
            max_lon: 0,
        },
    ];
    for k in 0..120 {
        let size = [10, 1_000, 100_000, 3_000_000, 60_000_000][k % 5];
        let (h, w) = (r.range(0, size), r.range(0, size));
        let s = r.range(-90_000_000, 90_000_000 - h);
        let west = r.range(-180_000_000, 180_000_000);
        let east = west + w;
        let east = if east > 180_000_000 {
            east - 360_000_000
        } else {
            east
        };
        v.push(Rect {
            min_lat: s,
            max_lat: s + h,
            min_lon: west,
            max_lon: east,
        });
    }
    v
}

fn ranges(groups: &[u32], n: usize, r: &mut Rng) -> Vec<Range<usize>> {
    let g = |k: usize| groups.get(k).map_or(n, |&x| x as usize);
    let a = r.range(0, n as i32) as usize;
    let b = r.range(0, n as i32 + 3) as usize;
    vec![
        0..n,
        g(0)..g(1),
        g(1)..g(2),
        g(2)..n,
        a..b,
        b..a,
        n..n + 5,
        a..a + 1,
    ]
}

fn check_queries(o: &Open<'_>, seed: u64) {
    let ix = o.ix.as_ref().unwrap();
    let n = o.v.len();
    let exact: Vec<Rect> = (0..n)
        .map(|p| polygon_box(&o.v, &o.a, p).unwrap())
        .collect();
    // A stored leaf moves each edge out by at most two quanta of its parent, and no parent is
    // wider than the root, the whole world here.
    let slack = 2 * (360_000_000 / QUANTA as i32 + 1);
    let mut r = Rng(seed);
    let groups = groups_of(o);
    for q in queries(&mut r) {
        for range in ranges(&groups, n, &mut r) {
            let mut got = Vec::new();
            let _ = ix.intersecting(q, range.clone(), |p| {
                got.push(p);
                ControlFlow::Continue(())
            });
            got.sort_unstable();
            let len = got.len();
            got.dedup();
            assert_eq!(got.len(), len, "a polygon twice for {q:?}");
            let inside = range.start..range.end.min(n);
            for p in inside.clone() {
                let hit = got.binary_search(&p).is_ok();
                if q.min_lat <= q.max_lat && meets(&exact[p], &q) {
                    assert!(hit, "polygon {p} dropped from {q:?} in {range:?}");
                }
                if hit {
                    assert!(
                        q.min_lat <= q.max_lat && meets(&grow_by(&exact[p], slack), &q),
                        "polygon {p} {:?} far from {q:?}",
                        exact[p]
                    );
                }
            }
            assert!(got.iter().all(|p| inside.contains(p)), "outside {range:?}");
            // Only the subtrees over the range are read: one polygon costs one path at most
            // (two across the antimeridian, which splits the query).
            if range.len() == 1 && range.start < n {
                let paths = if q.min_lon > q.max_lon { 2 } else { 1 };
                assert!(
                    ix.nodes_read(q, range.clone()) <= paths * ix.depth(),
                    "{range:?} {q:?}"
                );
            }
            if range.start >= n {
                assert_eq!(ix.nodes_read(q, range.clone()), 0);
            }
            // Early stop: one visit, then no more.
            let mut calls = 0;
            let _ = ix.intersecting(q, range.clone(), |_| {
                calls += 1;
                ControlFlow::Break(())
            });
            assert_eq!(calls, got.len().min(1));
        }
    }
}

fn groups_of(o: &Open<'_>) -> Vec<u32> {
    (0..o.v.groups())
        .map(|g| o.v.group(g).unwrap().start as u32)
        .collect()
}

fn check_containing(o: &Open<'_>, m: &[Model], seed: u64) {
    let ix = o.ix.as_ref().unwrap();
    let n = o.v.len();
    let mut r = Rng(seed);
    let groups = groups_of(o);
    for k in 0..300 {
        let p = match k % 4 {
            // On a corner or an edge of a model rectangle, where the border rule decides.
            0 => {
                let [w, s, e, nn] = m[r.range(0, n as i32 - 1) as usize];
                at(
                    [s, nn, r.range(s, nn)][k / 4 % 3],
                    [w, e, r.range(w, e)][k / 3 % 3],
                )
            }
            1 => at(r.range(-900_000_000, 900_000_000), [W, -W][k / 4 % 2]),
            _ => at(r.range(-900_000_000, 900_000_000), r.range(-W, W)),
        };
        // The border rule in closed form for a rectangle: the point is read as p + (ε, ε²), so
        // it is inside when w ≤ x < e and s ≤ y < n; longitude 180 is read as −180.
        let x = if p.lon() == W { -W } else { p.lon() };
        let holding: Vec<usize> = (0..n)
            .filter(|&i| {
                let [w, s, e, nn] = m[i];
                w <= x && x < e && s <= p.lat() && p.lat() < nn
            })
            .collect();
        // And `contains` agrees on a sample.
        for &i in holding.iter().take(4) {
            assert!(o.v.contains(&o.a, i, p));
        }
        for range in ranges(&groups, n, &mut r) {
            let mut got = Vec::new();
            let _ = ix.containing(&o.v, &o.a, p, range.clone(), |i| {
                got.push(i);
                ControlFlow::Continue(())
            });
            got.sort_unstable();
            let want: Vec<usize> = holding
                .iter()
                .copied()
                .filter(|i| range.contains(i))
                .collect();
            assert_eq!(got, want, "containing {p} in {range:?}");
        }
    }
}

#[test]
fn queries_match_brute_force() {
    for (seed, node, order) in [(1, 4, true), (2, 9, false), (3, 16, true), (4, 32, false)] {
        let (m, groups) = model(seed, order);
        let bytes = pack(&m, &groups, node, None);
        let p = Pack::open(&bytes).unwrap();
        let o = open(&p);
        o.v.check(&o.a, m.len()).unwrap();
        let ix = o.ix.as_ref().unwrap();
        ix.check(&o.v, &o.a).unwrap();
        assert_eq!((ix.len(), ix.node_size()), (m.len(), node));
        assert!(ix.depth() >= 3, "deep enough: {}", ix.depth());
        check_queries(&o, seed);
        check_containing(&o, &m, seed);
    }
}

/// The index's columns, decoded.
fn columns(body: &[u8]) -> [Vec<u32>; 3] {
    let t = Table::parse(body).unwrap();
    let all = |c: Packed<'_>| (0..c.len()).map(|i| c.get(i).unwrap()).collect::<Vec<_>>();
    [NODE_SIZE, ROOT, BOXES].map(|id| all(t.column(id).unwrap()))
}

fn body_of(cols: &[Vec<u32>; 3]) -> Vec<u8> {
    let enc: Vec<Vec<u8>> = cols.iter().map(|c| encode_auto(c)).collect();
    table(&[(NODE_SIZE, &enc[0][..]), (ROOT, &enc[1]), (BOXES, &enc[2])]).unwrap()
}

#[test]
fn check_finds_every_wrong_offset() {
    let (m, groups) = model(7, true);
    let boxes: Vec<Rect> = m
        .iter()
        .map(|&[w, s, e, n]| {
            let mut b = Rect::around_e7(s, w);
            b.grow(&Rect::around_e7(n, e));
            b
        })
        .collect();
    let good = poly_index::encode(&boxes, 8).unwrap();
    let cols = columns(&good);
    // A decoded round trip is the writer's bytes.
    assert_eq!(body_of(&cols), good);
    let leaves = 4 * m.len();
    let mut r = Rng(9);
    // Leaves, level-0 boxes and the boxes above: any one offset moved by one is refused.
    for at_box in [0, 1, leaves - 1, leaves, leaves + 3, cols[2].len() - 1]
        .into_iter()
        .chain((0..40).map(|_| r.range(0, cols[2].len() as i32 - 1) as usize))
    {
        let mut bad = cols.clone();
        let v = &mut bad[2][at_box];
        *v = if *v == 0 { 1 } else { *v - 1 };
        let bytes = pack(&m, &groups, 8, Some(&body_of(&bad)));
        let p = Pack::open(&bytes).unwrap();
        let o = open(&p);
        let ix = o.ix.unwrap();
        assert!(
            matches!(
                ix.check(&o.v, &o.a),
                Err(Error::Payload { column: BOXES, .. })
            ),
            "offset {at_box} moved"
        );
    }
    // A root one unit wider is refused: its children's offsets no longer fit it, or it is not
    // their bounds.
    let mut bad = cols.clone();
    bad[1][1] += 1;
    let bytes = pack(&m, &groups, 8, Some(&body_of(&bad)));
    let p = Pack::open(&bytes).unwrap();
    let o = open(&p);
    assert!(matches!(
        o.ix.unwrap().check(&o.v, &o.a),
        Err(Error::Payload {
            column: ROOT | BOXES,
            ..
        })
    ));
    // Missing the leaf level: the box count no longer fits the polygons.
    let mut bad = cols.clone();
    bad[2].drain(..leaves);
    let bytes = pack(&m, &groups, 8, Some(&body_of(&bad)));
    let p = Pack::open(&bytes).unwrap();
    assert!(matches!(
        open(&p).ix,
        Err(Error::Payload { column: BOXES, .. })
    ));
}

#[test]
fn edge_cases() {
    // No polygons, one polygon, and the writer's refusals.
    assert!(poly_index::encode(&[], 3).is_err());
    assert!(poly_index::encode(&[Rect::EMPTY], 8).is_err());
    let one = [[850_000_000, 270_000_000, 850_100_000, 270_100_000]];
    let bytes = pack(&one, &[0], 4, None);
    let p = Pack::open(&bytes).unwrap();
    let o = open(&p);
    let ix = o.ix.as_ref().unwrap();
    ix.check(&o.v, &o.a).unwrap();
    assert_eq!(ix.depth(), 1);
    let mut got = Vec::new();
    let _ = ix.containing(&o.v, &o.a, at(270_050_000, 850_050_000), 0..1, |i| {
        got.push(i);
        ControlFlow::Continue(())
    });
    assert_eq!(got, [0]);
    let empty = poly_index::encode(&[], 16).unwrap();
    let cols = columns(&empty);
    assert!(cols[1].is_empty() && cols[2].is_empty());
}

/// `Polygons::within` (D-149) against the exact distance to each rectangle on the same local
/// plane: inside at any radius, near when within it, never when beyond; rectangles on the
/// antimeridian are compared by wrapped longitude.
#[test]
fn within_matches_the_distance_to_each_rectangle() {
    let (m, groups) = model(11, true);
    let bytes = pack(&m, &groups, 9, None);
    let p = Pack::open(&bytes).unwrap();
    let o = open(&p);
    let mut r = Rng(99);
    let (mut near, mut far, mut inside) = (0, 0, 0);
    for _ in 0..4_000 {
        let k = (r.next() % m.len() as u64) as usize;
        let [w, s, e, n] = m[k];
        // A point around the rectangle: inside, near an edge or well outside.
        let span = (e - w).max(n - s).max(1_000);
        let lat = r.range((s - span).max(-850_000_000), (n + span).min(850_000_000));
        let lon = r.range((w - span).max(-W), (e + span).min(W - 1));
        let at = at(lat, lon);
        let kx = (f64::from(lat) * 1e-7).to_radians().cos();
        let wrap = |d: i64| {
            if d > 1_800_000_000 {
                d - 3_600_000_000
            } else if d < -1_800_000_000 {
                d + 3_600_000_000
            } else {
                d
            }
        };
        let dx = if lon < w {
            wrap(i64::from(w) - i64::from(lon))
        } else if lon > e {
            wrap(i64::from(lon) - i64::from(e))
        } else {
            0
        };
        let dy = if lat < s {
            s - lat
        } else if lat > n {
            lat - n
        } else {
            0
        };
        let d = ((dx as f64 * kx).powi(2) + f64::from(dy).powi(2)).sqrt() * 1.111_95;
        for r_cm in [0_u32, 1_000, 100_000, 10_000_000] {
            let got = o.v.within(&o.a, k, at, r_cm);
            if d == 0.0 && (lon > w && lon < e && lat > s && lat < n) {
                assert!(got, "inside {k} at {at:?}");
                inside += 1;
            } else if d < f64::from(r_cm) * 0.999 {
                assert!(got, "{d} cm within {r_cm} cm of {k} at {at:?}");
                near += 1;
            } else if d > f64::from(r_cm) * 1.001 + 2.0 {
                assert!(!got, "{d} cm beyond {r_cm} cm of {k} at {at:?}");
                far += 1;
            }
        }
    }
    assert!(
        inside > 100 && near > 100 && far > 100,
        "{inside} {near} {far}"
    );
}

/// `within_chunked` gives `within`'s answer: star-shaped rings of 40 to 900 vertices (many
/// chunks per arc, every chunk boundary inside some ring), half with a hole, some touching the
/// antimeridian with points on its other side, two arcs per outer ring; points around each, at radii from 0 to 1,000 km.
#[test]
fn within_chunked_matches_within() {
    let mut r = Rng(7);
    let mut arc_pts: Vec<Vec<Point<E7>>> = Vec::new();
    let mut polys: Vec<(Vec<u32>, Option<u32>, (i32, i32), i32)> = Vec::new();
    for k in 0..120 {
        let n = r.range(40, 900) as usize;
        let rad = r.range(10_000, 30_000_000);
        let lat0 = r.range(-800_000_000 + rad, 800_000_000 - rad);
        let lon0 = if k % 9 == 0 {
            W - rad / 2 * 3 - 10
        } else {
            r.range(-W + rad * 2, W - rad * 2)
        };
        let ring = |rad: i32, n: usize, r: &mut Rng, rev: bool| -> Vec<Point<E7>> {
            let mut v: Vec<Point<E7>> = (0..n)
                .map(|i| {
                    let t = i as f64 / n as f64 * std::f64::consts::TAU;
                    let rr = f64::from(rad) * (0.05 + 0.95 * (r.next() % 1000) as f64 / 1000.0);
                    let lat = (f64::from(lat0) + rr * t.sin()) as i32;
                    at(lat, lon0 + (rr * t.cos() * 1.5) as i32)
                })
                .collect();
            v.dedup();
            if rev {
                v.reverse();
            }
            v.push(v[0]);
            v
        };
        let outer = ring(rad, n, &mut r, false);
        // Two arcs: split the closed ring at a middle vertex.
        let mid = outer.len() / 2;
        let a0 = arc_pts.len() as u32;
        arc_pts.push(outer[..=mid].to_vec());
        arc_pts.push(outer[mid..].to_vec());
        let hole = (k % 2 == 0).then(|| {
            arc_pts.push(ring(rad / 40, 60, &mut r, true));
            arc_pts.len() as u32 - 1
        });
        // The label between the hole (at most rad / 40) and the outer ring (about 0.05 rad at least).
        polys.push((
            vec![arc_ref(a0, false), arc_ref(a0 + 1, false)],
            hole,
            (lat0 + rad / 1000 * 35, lon0),
            rad,
        ));
    }
    let pts: Vec<&[Point<E7>]> = arc_pts.iter().map(Vec::as_slice).collect();
    let arcs_body = arcs::encode(&ArcColumns {
        arcs: &pts,
        ..ArcColumns::default()
    })
    .unwrap();
    let holes: Vec<Option<[u32; 1]>> = polys
        .iter()
        .map(|p| p.1.map(|h| [arc_ref(h, false)]))
        .collect();
    let ring_lists: Vec<Vec<&[u32]>> = polys
        .iter()
        .zip(&holes)
        .map(|(p, h)| {
            let mut v: Vec<&[u32]> = vec![&p.0[..]];
            v.extend(h.as_ref().map(|h| &h[..]));
            v
        })
        .collect();
    let parts: Vec<[&[&[u32]]; 1]> = ring_lists.iter().map(|r| [&r[..]]).collect();
    let input: Vec<PolygonInput<'_>> = polys
        .iter()
        .zip(&parts)
        .enumerate()
        .map(|(i, (p, parts))| PolygonInput {
            row: i as u32,
            parts: &parts[..],
            label: at(p.2.0, p.2.1),
            area_ha: 1,
        })
        .collect();
    let poly_body = polygons::encode(&input, &[]).unwrap();
    let mut wr = Writer::new("np", "places", "2026.11.0").unwrap();
    wr.section_bytes(Kind::ARCS, Encoding::TABLE, pts.len() as u64, &arcs_body)
        .unwrap();
    wr.section_bytes(
        Kind::POLYGONS,
        Encoding::TABLE,
        polys.len() as u64,
        &poly_body,
    )
    .unwrap();
    let bytes = wr
        .layout(false)
        .unwrap()
        .assemble(&[&arcs_body, &poly_body])
        .unwrap();
    let p = Pack::open(&bytes).unwrap();
    let a = Arcs::open(&p.section(Kind::ARCS).unwrap()).unwrap();
    let v = Polygons::open(&p.section(Kind::POLYGONS).unwrap()).unwrap();
    v.check(&a, polys.len()).unwrap();
    let chunks = ArcChunks::build(&a);
    // Every segment once: the chunks of an arc cover its vertices, sharing ends.
    for k in 0..a.len() {
        let c = chunks.of(k);
        let range = a.range(k).unwrap();
        assert_eq!(c.first().unwrap().start as usize, range.start);
        assert_eq!(c.last().unwrap().end as usize, range.end);
        assert!(c.windows(2).all(|w| w[0].end == w[1].start + 1));
    }
    let (mut yes, mut no) = (0, 0);
    for _ in 0..30_000 {
        let k = (r.next() % polys.len() as u64) as usize;
        let (lat0, lon0) = polys[k].2;
        // Mostly around the star (its spikes make chunks that span the point with crossings on
        // both sides), some far, wrapped across the antimeridian.
        let rad = polys[k].3;
        let (dlat, dlon) = if r.next().is_multiple_of(10) {
            (40_000_000, 60_000_000)
        } else {
            (rad / 5 * 6, rad / 5 * 9)
        };
        let lat = (lat0 + r.range(-dlat, dlat)).clamp(-899_999_999, 899_999_999);
        let mut lon = i64::from(lon0) + i64::from(r.range(-dlon, dlon));
        if lon > i64::from(W) {
            lon -= 3_600_000_000;
        }
        let lon = lon.max(-i64::from(W)) as i32;
        let q = at(lat, lon);
        for r_cm in [0_u32, 100_000, 3_000_000, 100_000_000] {
            let want = v.within(&a, k, q, r_cm);
            assert_eq!(
                v.within_chunked(&a, &chunks, k, q, r_cm),
                want,
                "{k} at {q:?}, {r_cm} cm"
            );
            if want { yes += 1 } else { no += 1 }
        }
    }
    assert!(yes > 10_000 && no > 10_000, "{yes} {no}");
}

/// An edge across the meridian opposite the point is far from it: a rectangle 200° wide
/// (longitudes −100° to 100°, latitudes 10° to 20°) and a point at longitude −170°, 10 m north
/// of its southern edge's latitude. Each end wrapped alone around the point puts the edge's
/// ends at +70° and −90°, a segment through the point; the answer is 70° away.
#[test]
fn an_edge_across_the_opposite_meridian_is_far() {
    let m = [[-1_000_000_000, 100_000_000, 1_000_000_000, 200_000_000]];
    let bytes = pack(&m, &[], 4, None);
    let p = Pack::open(&bytes).unwrap();
    let o = open(&p);
    let chunks = ArcChunks::build(&o.a);
    let q = at(100_000_900, -1_700_000_000);
    for r_cm in [0, 10_000, 100_000] {
        assert!(!o.v.within(&o.a, 0, q, r_cm), "{r_cm} cm");
        assert!(
            !o.v.within_chunked(&o.a, &chunks, 0, q, r_cm),
            "{r_cm} cm, chunked"
        );
    }
    // A point 10 m south of the edge, 100° east of the first: within 100 m, not within 1 m.
    let q = at(99_999_100, -700_000_000);
    assert!(o.v.within(&o.a, 0, q, 10_000) && o.v.within_chunked(&o.a, &chunks, 0, q, 10_000));
    assert!(!o.v.within(&o.a, 0, q, 100) && !o.v.within_chunked(&o.a, &chunks, 0, q, 100));
}
