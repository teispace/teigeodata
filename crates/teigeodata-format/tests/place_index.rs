//! `PLACE_INDEX` against brute force: box queries exactly, nearest places by distance, for every
//! node size, tiny and large packs, clusters, duplicates, the poles and the antimeridian; then
//! every rejection and the canonical check.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::many_single_char_names,
    clippy::similar_names,
    reason = "tests: a failure panics with its message; inputs are cut from a 64-bit generator"
)]

use std::ops::ControlFlow;

use teigeodata_core::bitpack::encode_auto;
use teigeodata_core::table::encode as table;
use teigeodata_core::{E6, PlaceKind, Point, hilbert};
use teigeodata_format::place_index::{self, EARTH_RADIUS_M, Neighbour, PlaceIndex, Rect};
use teigeodata_format::places::{self, PlaceColumns, Places};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (u64::from(hi.abs_diff(lo)) + 1)) as i32
    }
}

const DEG: i32 = 1_000_000;

/// Points of several shapes, in Hilbert order: world-uniform, a dense cluster, duplicates, and
/// points on the poles and the antimeridian.
fn points(n: usize, seed: u64) -> Vec<Point<E6>> {
    let mut r = Rng(seed);
    let mut v: Vec<Point<E6>> = (0..n)
        .map(|i| {
            let (lat, lon) = match i % 5 {
                0 => (r.range(-90 * DEG, 90 * DEG), r.range(-180 * DEG, 180 * DEG)),
                1 | 2 => (r.range(26 * DEG, 31 * DEG), r.range(80 * DEG, 89 * DEG)),
                3 => (27_700_000, 85_300_000 + r.range(0, 3)),
                _ => match r.next() % 4 {
                    0 => (90 * DEG, r.range(-180 * DEG, 180 * DEG)),
                    1 => (r.range(-90 * DEG, 90 * DEG), 180 * DEG),
                    2 => (r.range(-60 * DEG, 60 * DEG), -180 * DEG + r.range(0, 1000)),
                    _ => (-90 * DEG, 0),
                },
            };
            Point::from_units(lat, lon).unwrap()
        })
        .collect();
    v.sort_by_key(|p| (hilbert(*p), *p));
    v
}

/// A pack with PLACES and a `PLACE_INDEX` of node size `node`.
fn pack(pts: &[Point<E6>], node: usize) -> Vec<u8> {
    let kinds = vec![PlaceKind::Settlement; pts.len()];
    let places = places::encode(&PlaceColumns {
        points: pts,
        kinds: &kinds,
        ..PlaceColumns::default()
    })
    .unwrap();
    let index = place_index::encode(pts, node).unwrap();
    with_index(&places, pts.len() as u64, &index, pts.len() as u64)
}

fn with_index(places: &[u8], rows: u64, index: &[u8], items: u64) -> Vec<u8> {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.section_bytes(Kind::PLACES, Encoding::TABLE, rows, places)
        .unwrap();
    w.section_bytes(Kind::PLACE_INDEX, Encoding::TABLE, items, index)
        .unwrap();
    w.layout(false).unwrap().assemble(&[places, index]).unwrap()
}

fn open(bytes: &[u8]) -> Result<PlaceIndex<'_>, Error> {
    let pack = Pack::open(bytes)?;
    let places = Places::open(&pack.section(Kind::PLACES).unwrap())?;
    PlaceIndex::open(&pack.section(Kind::PLACE_INDEX).unwrap(), places)
}

fn in_rect(p: Point<E6>, q: &Rect) -> bool {
    let lon_ok = if q.min_lon <= q.max_lon {
        q.min_lon <= p.lon() && p.lon() <= q.max_lon
    } else {
        p.lon() >= q.min_lon || p.lon() <= q.max_lon
    };
    q.min_lat <= p.lat() && p.lat() <= q.max_lat && lon_ok
}

fn query_rects(r: &mut Rng) -> Vec<Rect> {
    let mut v = vec![
        Rect {
            min_lat: -90 * DEG,
            max_lat: 90 * DEG,
            min_lon: -180 * DEG,
            max_lon: 180 * DEG,
        },
        // Across the antimeridian.
        Rect {
            min_lat: -70 * DEG,
            max_lat: 70 * DEG,
            min_lon: 170 * DEG,
            max_lon: -170 * DEG,
        },
        // One point, a duplicate.
        Rect {
            min_lat: 27_700_000,
            max_lat: 27_700_000,
            min_lon: 85_300_001,
            max_lon: 85_300_001,
        },
        // Empty: south above north.
        Rect {
            min_lat: 1,
            max_lat: 0,
            min_lon: 0,
            max_lon: 0,
        },
    ];
    for _ in 0..60 {
        let (a, b) = (r.range(-90 * DEG, 90 * DEG), r.range(-90 * DEG, 90 * DEG));
        let (c, d) = (
            r.range(-180 * DEG, 180 * DEG),
            r.range(-180 * DEG, 180 * DEG),
        );
        v.push(Rect {
            min_lat: a.min(b),
            max_lat: a.max(b),
            min_lon: c,
            max_lon: d,
        });
        let (lat, lon) = (r.range(25 * DEG, 32 * DEG), r.range(79 * DEG, 90 * DEG));
        let w = r.range(0, DEG / 2);
        v.push(Rect {
            min_lat: lat - w,
            max_lat: lat + w,
            min_lon: lon - w,
            max_lon: lon + w,
        });
    }
    v
}

fn haversine_m(a: Point<E6>, b: Point<E6>) -> f64 {
    let (p1, p2) = (a.lat_degrees().to_radians(), b.lat_degrees().to_radians());
    let dl = (a.lon_degrees() - b.lon_degrees()).to_radians();
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.clamp(0.0, 1.0).sqrt().asin()
}

fn check_queries(pts: &[Point<E6>], ix: &PlaceIndex<'_>, seed: u64) {
    let mut r = Rng(seed);
    for q in query_rects(&mut r) {
        let mut got = Vec::new();
        let flow = ix.within(q, |row| {
            got.push(row);
            ControlFlow::Continue(())
        });
        assert_eq!(flow, ControlFlow::Continue(()));
        got.sort_unstable();
        let want: Vec<usize> = (0..pts.len()).filter(|&i| in_rect(pts[i], &q)).collect();
        assert_eq!(got, want, "box {q:?}");
        // Early stop: exactly one row visited.
        let mut seen = 0;
        let stop = ix.within(q, |_| {
            seen += 1;
            ControlFlow::Break(())
        });
        assert_eq!(
            (seen, stop.is_break()),
            (usize::from(!want.is_empty()), !want.is_empty())
        );
    }
    for _ in 0..40 {
        let at =
            Point::<E6>::from_units(r.range(-90 * DEG, 90 * DEG), r.range(-180 * DEG, 180 * DEG))
                .unwrap();
        let near =
            Point::<E6>::from_units(r.range(26 * DEG, 31 * DEG), r.range(80 * DEG, 89 * DEG))
                .unwrap();
        // The last is on the duplicates, where many rows tie at distance 0.
        let dup = Point::<E6>::from_units(27_700_000, 85_300_001).unwrap();
        for q in [at, near, pts.first().copied().unwrap_or(at), dup] {
            let mut brute: Vec<f64> = pts.iter().map(|&p| haversine_m(q, p)).collect();
            brute.sort_by(f64::total_cmp);
            for k in [1, 7, 50] {
                let mut out = vec![Neighbour::default(); k];
                let n = ix.nearest(q, &mut out);
                assert_eq!(n, k.min(pts.len()));
                for (j, nb) in out[..n].iter().enumerate() {
                    let d = nb.meters();
                    assert!(
                        (d - haversine_m(q, pts[nb.row])).abs() < 1e-6,
                        "row distance"
                    );
                    assert!(
                        (d - brute[j]).abs() < 1e-6,
                        "{j}th of {k} from {q:?}: {d} vs {}",
                        brute[j]
                    );
                }
                // Ties go to the lower row: a row at the same point as a result, below it,
                // is in the result too.
                let rows: Vec<usize> = out[..n].iter().map(|nb| nb.row).collect();
                for &row in &rows {
                    for lower in 0..row {
                        assert!(
                            pts[lower] != pts[row] || rows.contains(&lower),
                            "{k} from {q:?}: row {row} taken before the tied row {lower}"
                        );
                    }
                }
                assert!(
                    out[..n]
                        .windows(2)
                        .all(|w| (w[0].haversine, w[0].row) < (w[1].haversine, w[1].row))
                );
            }
        }
    }
}

#[test]
fn queries_match_brute_force() {
    for (n, seed) in [
        (0, 1),
        (1, 2),
        (3, 3),
        (16, 4),
        (17, 5),
        (257, 6),
        (5_000, 7),
    ] {
        let pts = points(n, seed);
        for node in [4, 8, 16, 32] {
            let bytes = pack(&pts, node);
            let ix = open(&bytes).unwrap();
            ix.check().unwrap();
            assert_eq!(ix.node_size(), node);
            check_queries(&pts, &ix, seed * 31 + node as u64);
        }
    }
}

#[test]
fn a_larger_pack() {
    let pts = points(60_000, 99);
    let bytes = pack(&pts, 16);
    let ix = open(&bytes).unwrap();
    ix.check().unwrap();
    assert_eq!(ix.depth(), 4);
    check_queries(&pts, &ix, 7);
}

#[test]
fn refusals() {
    let pts = points(300, 11);
    let kinds = vec![PlaceKind::Settlement; pts.len()];
    let places = places::encode(&PlaceColumns {
        points: &pts,
        kinds: &kinds,
        ..PlaceColumns::default()
    })
    .unwrap();
    let rows = pts.len() as u64;
    let good = place_index::encode(&pts, 16).unwrap();
    let problem = |index: &[u8], items: u64| match open(&with_index(&places, rows, index, items)) {
        Err(Error::Payload {
            column, problem, ..
        }) => (column, problem),
        other => panic!("accepted: {:?}", other.map(|i| i.depth())),
    };
    assert_eq!(
        problem(&good, rows + 1).1,
        "the item count is not the number of places"
    );
    let col = |v: &[u32]| encode_auto(v);
    // 300 rows at node 16: levels of 19, 2 and 1 boxes; 21 are stored, 4 offsets each.
    let pack_of = |node: u32, extra: Option<u16>, root: usize, boxes: usize| {
        let mut cols = vec![
            (1, col(&[node])),
            (2, col(&vec![5; root])),
            (3, col(&vec![5; boxes])),
        ];
        if let Some(id) = extra {
            cols.push((id, col(&[1])));
        }
        let refs: Vec<(u16, &[u8])> = cols.iter().map(|(i, c)| (*i, &c[..])).collect();
        table(&refs).unwrap()
    };
    let len = "the column length differs from the row count";
    assert_eq!(problem(&pack_of(16, None, 4, 80), rows), (3, len));
    assert_eq!(problem(&pack_of(16, None, 3, 84), rows), (2, len));
    let range = "the node size is out of range";
    assert_eq!(problem(&pack_of(3, None, 4, 84), rows), (1, range));
    assert_eq!(problem(&pack_of(33, None, 4, 84), rows), (1, range));
    assert_eq!(
        problem(&pack_of(16, Some(9), 4, 84), rows).1,
        "an unknown column"
    );
    // The right shape opens; its made-up boxes fail the check.
    let made_up = pack_of(16, None, 4, 84);
    let bytes = with_index(&places, rows, &made_up, rows);
    assert!(open(&bytes).unwrap().check().is_err());
    assert!(place_index::encode(&pts, 3).is_err());
    assert!(place_index::encode(&pts, 33).is_err());
}

/// The check refuses any change of the root or of a stored offset, up or down by one, including
/// offsets that decode to the same edge on a narrow parent.
#[test]
fn the_check_is_canonical() {
    let pts = points(700, 21);
    let kinds = vec![PlaceKind::Settlement; pts.len()];
    let places = places::encode(&PlaceColumns {
        points: &pts,
        kinds: &kinds,
        ..PlaceColumns::default()
    })
    .unwrap();
    let rows = pts.len() as u64;
    // Rebuild the index columns by hand, then change one value at a time.
    let good = place_index::encode(&pts, 8).unwrap();
    let bytes = with_index(&places, rows, &good, rows);
    let ix = open(&bytes).unwrap();
    ix.check().unwrap();
    let pack = Pack::open(&bytes).unwrap();
    let t = teigeodata_core::table::Table::parse(pack.section(Kind::PLACE_INDEX).unwrap().bytes())
        .unwrap();
    let cols: Vec<(u16, Vec<u32>)> = (1..=3)
        .map(|id| {
            let c = t.column(id).unwrap();
            let mut v = vec![0; c.len()];
            for (i, x) in c.values(0..c.len()).enumerate() {
                v[i] = x;
            }
            (id, v)
        })
        .collect();
    let mut r = Rng(5);
    for _ in 0..400 {
        // The root or a stored offset.
        let col = 1 + (r.next() % 2) as usize;
        let at = (r.next() % cols[col].1.len() as u64) as usize;
        let delta = if r.next().is_multiple_of(2) {
            1
        } else {
            u32::MAX
        };
        let mut edited = cols.clone();
        edited[col].1[at] = edited[col].1[at].wrapping_add(delta);
        let enc: Vec<(u16, Vec<u8>)> = edited.iter().map(|(i, v)| (*i, encode_auto(v))).collect();
        let refs: Vec<(u16, &[u8])> = enc.iter().map(|(i, c)| (*i, &c[..])).collect();
        let body = table(&refs).unwrap();
        let bytes = with_index(&places, rows, &body, rows);
        let ix = open(&bytes).unwrap();
        assert!(
            ix.check().is_err(),
            "column {col} value {at} changed by {delta} passes"
        );
    }
}
