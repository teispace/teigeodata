//! PLACES and HIERARCHY through the writer and the reader: round trips against a model, every
//! rejection, and no panic on mutated payloads.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; inputs are cut from a 64-bit generator"
)]

use teigeodata_core::bitpack::{Coding, bias, encode as packed};
use teigeodata_core::table::encode as table;
use teigeodata_core::{E6, PlaceKind, Point, hilbert};
use teigeodata_format::hierarchy::{self, Hierarchy, MAX_DEPTH};
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

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A pack holding these sections, as `(kind, encoding, items, body)`.
fn pack_of(sections: &[(Kind, Encoding, u64, &[u8])]) -> Vec<u8> {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    for &(kind, enc, items, body) in sections {
        w.section_bytes(kind, enc, items, body).unwrap();
    }
    let bodies: Vec<&[u8]> = sections.iter().map(|s| s.3).collect();
    w.layout(false).unwrap().assemble(&bodies).unwrap()
}

fn places_of(bytes: &[u8], items: u64) -> Result<(), Error> {
    let p = pack_of(&[(Kind::PLACES, Encoding::TABLE, items, bytes)]);
    let pack = Pack::open(&p)?;
    let section = pack.section(Kind::PLACES).unwrap();
    Places::open(&section)?.check()
}

fn hierarchy_of(bytes: &[u8], items: u64) -> Result<(), Error> {
    let p = pack_of(&[(Kind::HIERARCHY, Encoding::TABLE, items, bytes)]);
    let pack = Pack::open(&p)?;
    let section = pack.section(Kind::HIERARCHY).unwrap();
    Hierarchy::open(&section)?.check()
}

fn problem(r: Result<(), Error>) -> (u16, &'static str) {
    match r {
        Err(Error::Payload {
            column, problem, ..
        }) => (column, problem),
        other => panic!("expected a payload error, got {other:?}"),
    }
}

/// A random country-like tree: a root, `a` level-1 units, `b` level-2 units, and settlements,
/// with rows in Hilbert order of their points, as the forge writes them.
struct Model {
    points: Vec<Point<E6>>,
    kinds: Vec<PlaceKind>,
    levels: Vec<u8>,
    classes: Vec<u32>,
    parents: Vec<Option<u32>>,
}

/// Hilbert index, point, kind, admin level, parent (by original index).
type Row = (u64, Point<E6>, PlaceKind, u8, Option<usize>);

fn model(n: usize, seed: u64) -> Model {
    let mut r = Rng(seed);
    let mut rows: Vec<Row> = Vec::with_capacity(n);
    for i in 0..n {
        let p = Point::<E6>::from_units(
            26_000_000 + r.below(4_000_000) as i32,
            80_000_000 + r.below(8_000_000) as i32,
        )
        .unwrap();
        let (kind, level, parent) = match i {
            0 => (PlaceKind::Country, 0, None),
            1..=7 => (PlaceKind::Admin, 1, Some(0)),
            8..=84 => (PlaceKind::Admin, 2, Some(1 + r.below(7) as usize)),
            _ if r.below(50) == 0 => (PlaceKind::Natural, 0, None),
            _ => (PlaceKind::Settlement, 0, Some(8 + r.below(77) as usize)),
        };
        rows.push((hilbert(p), p, kind, level, parent));
    }
    // Sort into Hilbert order and renumber the parents.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (rows[i].0, i));
    let mut new_row = vec![0; n];
    for (new, &old) in order.iter().enumerate() {
        new_row[old] = new;
    }
    Model {
        points: order.iter().map(|&i| rows[i].1).collect(),
        kinds: order.iter().map(|&i| rows[i].2).collect(),
        levels: order.iter().map(|&i| rows[i].3).collect(),
        classes: order
            .iter()
            .map(|&i| u32::from(rows[i].2 == PlaceKind::Settlement) * 3)
            .collect(),
        parents: order
            .iter()
            .map(|&i| rows[i].4.map(|p| new_row[p] as u32))
            .collect(),
    }
}

fn cols(m: &Model) -> PlaceColumns<'_> {
    PlaceColumns {
        points: &m.points,
        kinds: &m.kinds,
        admin_levels: &m.levels,
        classes: &m.classes,
        flags: &[],
    }
}

#[test]
fn places_round_trip() {
    for n in [0, 1, 85, 127, 128, 129, 5000] {
        let m = model(n.max(85), 11 + n as u64);
        let m = Model {
            points: m.points[..n].to_vec(),
            kinds: m.kinds[..n].to_vec(),
            levels: m.levels[..n].to_vec(),
            classes: m.classes[..n].to_vec(),
            parents: Vec::new(),
        };
        let body = places::encode(&cols(&m)).unwrap();
        let p = pack_of(&[(Kind::PLACES, Encoding::TABLE, n as u64, &body)]);
        let pack = Pack::open_verified(&p).unwrap();
        let places = Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
        places.check().unwrap();
        assert_eq!(places.len(), n);
        for row in 0..n {
            assert_eq!(places.point(row), Some(m.points[row]));
            assert_eq!(places.kind(row), Some(m.kinds[row]));
            assert_eq!(places.admin_level(row), Some(m.levels[row]));
            assert_eq!(places.class(row), Some(m.classes[row]));
            assert_eq!(places.flags(row), Some(0));
        }
        assert_eq!(places.point(n), None);
        assert_eq!(places.kind(n), None);
        assert_eq!(places.flags(n), None);
    }
}

#[test]
fn hierarchy_round_trip() {
    let m = model(5000, 5);
    let body = hierarchy::encode(&m.parents).unwrap();
    let p = pack_of(&[(Kind::HIERARCHY, Encoding::TABLE, 5000, &body)]);
    let pack = Pack::open_verified(&p).unwrap();
    let h = Hierarchy::open(&pack.section(Kind::HIERARCHY).unwrap()).unwrap();
    h.check().unwrap();
    assert_eq!(h.len(), 5000);
    for row in 0..5000 {
        assert_eq!(h.parent(row), m.parents[row].map(|p| p as usize));
        let want: Vec<u32> = (0..5000_u32)
            .filter(|&c| m.parents[c as usize] == Some(row as u32))
            .collect();
        assert_eq!(h.children(row).collect::<Vec<_>>(), want, "row {row}");
        let mut chain = Vec::new();
        let mut at = row;
        while let Some(p) = m.parents[at] {
            at = p as usize;
            chain.push(at);
        }
        assert_eq!(h.ancestors(row).collect::<Vec<_>>(), chain);
    }
    assert_eq!(h.parent(5000), None);
    assert_eq!(h.children(5000).len(), 0);
    // An empty hierarchy.
    let body = hierarchy::encode(&[]).unwrap();
    assert!(hierarchy_of(&body, 0).is_ok());
}

#[test]
fn writers_refuse_bad_input() {
    let m = model(300, 9);
    let mut unsorted = m.points.clone();
    unsorted.swap(3, 200);
    let bad = PlaceColumns {
        points: &unsorted,
        ..cols(&m)
    };
    assert!(matches!(places::encode(&bad), Err(Error::Write(_))));
    let short = PlaceColumns {
        kinds: &m.kinds[1..],
        ..cols(&m)
    };
    assert!(matches!(places::encode(&short), Err(Error::Write(_))));
    for parents in [
        vec![Some(1), Some(0)],          // a cycle
        vec![None, Some(1)],             // its own parent
        vec![None, Some(7)],             // out of range
        vec![Some(1), Some(2), Some(1)], // a cycle below the row
    ] {
        assert!(
            matches!(hierarchy::encode(&parents), Err(Error::Write(_))),
            "{parents:?}"
        );
    }
    // A chain of exactly MAX_DEPTH ancestors is allowed; one more is not.
    let chain = |len: usize| -> Vec<Option<u32>> {
        (0..len)
            .map(|i| i.checked_sub(1).map(|p| p as u32))
            .collect()
    };
    assert!(hierarchy::encode(&chain(MAX_DEPTH + 1)).is_ok());
    assert!(hierarchy::encode(&chain(MAX_DEPTH + 2)).is_err());
}

#[test]
fn open_refuses_the_wrong_section() {
    let m = model(200, 3);
    let body = places::encode(&cols(&m)).unwrap();
    // Wrong encoding, wrong row count, wrong kind.
    let p = pack_of(&[(Kind::PLACES, Encoding::RAW, 200, &body)]);
    let pack = Pack::open(&p).unwrap();
    assert!(Places::open(&pack.section(Kind::PLACES).unwrap()).is_err());
    assert_eq!(
        problem(places_of(&body, 199)),
        (places::LAT, "the column length differs from the row count")
    );
    let p = pack_of(&[(Kind::PLACES, Encoding::TABLE, 200, &body)]);
    let pack = Pack::open(&p).unwrap();
    assert!(Hierarchy::open(&pack.section(Kind::PLACES).unwrap()).is_err());
    // A missing required column, and an unknown one.
    let lat = packed(&[bias(0); 4], Coding::For);
    let only_lat = table(&[(places::LAT, &lat)]).unwrap();
    assert_eq!(
        problem(places_of(&only_lat, 4)),
        (places::LON, "a required column is missing")
    );
    let kind = packed(&[1; 4], Coding::For);
    let extra = table(&[
        (places::LAT, &lat),
        (places::LON, &lat),
        (places::KIND, &kind),
        (99, &kind),
    ])
    .unwrap();
    assert_eq!(problem(places_of(&extra, 4)), (0, "an unknown column"));
}

#[test]
fn places_check_finds_bad_values() {
    let at = |lat: i32, lon: i32| [bias(lat), bias(lon)];
    let build = |lats: &[u32], lons: &[u32], kinds: &[u32], extra: &[(u16, Vec<u32>)]| {
        let (a, b, c) = (
            packed(lats, Coding::For),
            packed(lons, Coding::For),
            packed(kinds, Coding::For),
        );
        let extra: Vec<(u16, Vec<u8>)> = extra
            .iter()
            .map(|(id, v)| (*id, packed(v, Coding::For)))
            .collect();
        let mut cols: Vec<(u16, &[u8])> =
            vec![(places::LAT, &a), (places::LON, &b), (places::KIND, &c)];
        cols.extend(extra.iter().map(|(id, v)| (*id, &v[..])));
        table(&cols).unwrap()
    };
    let [la, lo] = at(0, 0);
    // Off the grid (latitude above 90°).
    let [far, _] = at(91_000_000, 0);
    assert_eq!(
        problem(places_of(&build(&[far], &[lo], &[1], &[]), 1)),
        (places::LAT, "a point is off the grid")
    );
    // Out of Hilbert order.
    let p = [
        Point::<E6>::from_units(0, 0).unwrap(),
        Point::from_units(10_000_000, 5).unwrap(),
    ];
    let (first, second) = if hilbert(p[0]) < hilbert(p[1]) {
        (p[1], p[0])
    } else {
        (p[0], p[1])
    };
    let lats = [bias(first.lat()), bias(second.lat())];
    let lons = [bias(first.lon()), bias(second.lon())];
    assert_eq!(
        problem(places_of(&build(&lats, &lons, &[1, 1], &[]), 2)),
        (places::LAT, "rows are not in Hilbert order")
    );
    // Unknown kind; a level on a settlement; an all-0 optional column.
    assert_eq!(
        problem(places_of(&build(&[la], &[lo], &[0], &[]), 1)),
        (places::KIND, "an unknown kind")
    );
    assert_eq!(
        problem(places_of(
            &build(&[la], &[lo], &[3], &[(places::ADMIN_LEVEL, vec![1])]),
            1
        ))
        .0,
        places::ADMIN_LEVEL
    );
    assert_eq!(
        problem(places_of(
            &build(&[la], &[lo], &[1], &[(places::CLASS, vec![0])]),
            1
        )),
        (places::CLASS, "an optional column that is all 0 is stored")
    );
}

#[test]
fn hierarchy_check_finds_bad_values() {
    // Rows 0 ← 1 ← 2. Child lists that lie, and counts that disagree.
    let h = |parent: &[u32], ends: &[u32], children: &[u32]| {
        let (a, b, c) = (
            packed(parent, Coding::For),
            packed(ends, Coding::For),
            packed(children, Coding::For),
        );
        table(&[
            (hierarchy::PARENT, &a),
            (hierarchy::CHILD_END, &b),
            (hierarchy::CHILDREN, &c),
        ])
        .unwrap()
    };
    assert!(hierarchy_of(&h(&[0, 1, 2], &[1, 2, 2], &[1, 2]), 3).is_ok());
    // Row 0 with two children, read back.
    let two = h(&[0, 1, 1], &[2, 2, 2], &[1, 2]);
    let p = pack_of(&[(Kind::HIERARCHY, Encoding::TABLE, 3, &two)]);
    let pack = Pack::open(&p).unwrap();
    let tree = Hierarchy::open(&pack.section(Kind::HIERARCHY).unwrap()).unwrap();
    assert_eq!(tree.children(0).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(tree.children(1).len(), 0);
    tree.check().unwrap();
    // The same children, unsorted.
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 1, 1], &[2, 2, 2], &[2, 1]), 3)),
        (
            hierarchy::CHILDREN,
            "a child list is unsorted or names a non-child"
        )
    );
    // More children stored than counted.
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 1, 0], &[1, 1, 1], &[1, 2]), 3)).0,
        hierarchy::CHILD_END
    );
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 1, 2], &[1, 2, 2], &[2, 1]), 3)).0,
        hierarchy::CHILDREN
    );
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 1, 2], &[1, 2, 3], &[1, 2]), 3)).0,
        hierarchy::CHILD_END
    );
    // Two roots and one child: valid.
    assert!(hierarchy_of(&h(&[0, 1, 0], &[1, 1, 1], &[1]), 3).is_ok());
    // A parent named that does not list the row as a child.
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 1, 1], &[1, 1, 1], &[1]), 3)),
        (
            hierarchy::CHILDREN,
            "a row with a parent is missing from its children"
        )
    );
    // Its own parent, and a cycle 1 ↔ 2.
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 2, 0], &[0, 1, 1], &[1]), 3)),
        (
            hierarchy::PARENT,
            "a parent is out of range or the row itself"
        )
    );
    assert_eq!(
        problem(hierarchy_of(&h(&[0, 3, 2], &[0, 1, 2], &[2, 1]), 3)),
        (
            hierarchy::PARENT,
            "a chain of parents is a cycle or deeper than allowed"
        )
    );
}

#[test]
fn mutated_payloads_never_panic() {
    let tree = model(700, 21);
    let bodies = [
        (Kind::PLACES, places::encode(&cols(&tree)).unwrap()),
        (Kind::HIERARCHY, hierarchy::encode(&tree.parents).unwrap()),
    ];
    let mut rng = Rng(77);
    for i in 0..4000 {
        let (kind, body) = &bodies[i % 2];
        let mut b = body.clone();
        for _ in 0..=rng.below(3) {
            let at = rng.below(b.len() as u64) as usize;
            b[at] = rng.next() as u8;
        }
        let cut = b.len() - rng.below(8) as usize;
        let b = &b[..cut.min(b.len())];
        let p = pack_of(&[(*kind, Encoding::TABLE, 700, b)]);
        let pack = Pack::open(&p).unwrap();
        let s = pack.section(*kind).unwrap();
        if *kind == Kind::PLACES {
            if let Ok(pl) = Places::open(&s) {
                let _ = pl.check();
                for row in [0, 1, 350, 699, 700] {
                    let _ = (
                        pl.point(row),
                        pl.kind(row),
                        pl.admin_level(row),
                        pl.class(row),
                    );
                }
            }
        } else if let Ok(h) = Hierarchy::open(&s) {
            let _ = h.check();
            for row in [0, 1, 350, 699, 700] {
                let _ = (
                    h.parent(row),
                    h.children(row).count(),
                    h.ancestors(row).count(),
                );
            }
        }
    }
}
