//! IDMAP and REDIRECTS through the writer and the reader: round trips against a model, misses,
//! resolution, every rejection, and no panic on mutated payloads.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; inputs are cut from a 64-bit generator"
)]

use teigeodata_core::bitpack::{Coding, encode as packed};
use teigeodata_core::table::encode as table;
use teigeodata_core::{Namespace, PlaceId};
use teigeodata_format::idmap::{self, IdMap};
use teigeodata_format::redirects::{self, Fate, Redirects, Resolved, resolve};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn id(cc: [u8; 2], serial: u64) -> PlaceId {
    PlaceId::new(Namespace::from_country(cc).unwrap(), serial).unwrap()
}

fn pack_of(sections: &[(Kind, u64, &[u8])]) -> Vec<u8> {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    for &(kind, items, body) in sections {
        w.section_bytes(kind, Encoding::TABLE, items, body).unwrap();
    }
    let bodies: Vec<&[u8]> = sections.iter().map(|s| s.2).collect();
    w.layout(false).unwrap().assemble(&bodies).unwrap()
}

/// Mostly `np` serials 1..n in shuffled order, a few from `in`, and some big serials (above
/// 2^32, so the high column is not constant).
fn ids(n: usize, seed: u64) -> Vec<PlaceId> {
    let mut r = Rng(seed);
    let mut v: Vec<PlaceId> = (1..=n as u64)
        .map(|s| match s % 97 {
            0 => id(*b"in", s),
            1 => id(*b"np", s + (1 << 40)),
            _ => id(*b"np", s),
        })
        .collect();
    for i in (1..v.len()).rev() {
        v.swap(i, (r.next() % (i as u64 + 1)) as usize);
    }
    v
}

fn open_map(bytes: &[u8], rows: u64, f: impl FnOnce(&IdMap<'_>)) {
    let p = pack_of(&[(Kind::IDMAP, rows, bytes)]);
    let pack = Pack::open_verified(&p).unwrap();
    f(&IdMap::open(&pack.section(Kind::IDMAP).unwrap()).unwrap());
}

#[test]
fn idmap_round_trip_and_misses() {
    for n in [0, 1, 2, 127, 128, 129, 1000, 20_000] {
        let v = ids(n, 3 + n as u64);
        let body = idmap::encode(&v).unwrap();
        open_map(&body, n as u64, |m| {
            m.check().unwrap();
            assert_eq!(m.len(), n);
            for (row, &place) in v.iter().enumerate() {
                assert_eq!(m.id(row), Some(place));
                assert_eq!(m.row(place), Some(row), "{place}");
            }
            assert_eq!(m.id(n), None);
            // Misses: next to every present serial, below, above, another namespace.
            for s in [
                0,
                n as u64 + 1,
                n as u64 + 2,
                1 << 41,
                (1 << 40) + 2 * n as u64,
            ] {
                if let Some(missing) = PlaceId::new(Namespace::from_country(*b"np").unwrap(), s)
                    && !v.contains(&missing)
                {
                    assert_eq!(m.row(missing), None, "{missing}");
                }
            }
            assert_eq!(m.row(id(*b"ae", 5)), None);
        });
    }
}

#[test]
fn redirects_resolve() {
    let live = ids(500, 9);
    let map = idmap::encode(&live).unwrap();
    let (gone, merged_into) = (id(*b"np", 9_000), live[17]);
    let pairs = [
        (id(*b"np", 8_000), Some(merged_into)),
        (gone, None),
        (id(*b"in", 7_000), Some(live[3])),
    ];
    let red = redirects::encode(&pairs).unwrap();
    let bytes = pack_of(&[(Kind::IDMAP, 500, &map), (Kind::REDIRECTS, 3, &red)]);
    let pack = Pack::open_verified(&bytes).unwrap();
    let ids_view = IdMap::open(&pack.section(Kind::IDMAP).unwrap()).unwrap();
    let red_view = Redirects::open(&pack.section(Kind::REDIRECTS).unwrap()).unwrap();
    red_view.check(&ids_view).unwrap();
    assert_eq!(red_view.len(), 3);
    assert_eq!(red_view.get(gone), Some(Fate::Tombstone));
    assert_eq!(
        red_view.get(id(*b"np", 8_000)),
        Some(Fate::Redirect(merged_into))
    );
    assert_eq!(red_view.get(live[0]), None);
    assert_eq!(
        resolve(&ids_view, Some(&red_view), live[40]),
        Resolved::Row(40)
    );
    assert_eq!(
        resolve(&ids_view, Some(&red_view), id(*b"np", 8_000)),
        Resolved::Moved(merged_into, 17)
    );
    assert_eq!(resolve(&ids_view, Some(&red_view), gone), Resolved::Retired);
    assert_eq!(
        resolve(&ids_view, Some(&red_view), id(*b"np", 9_001)),
        Resolved::Unknown
    );
    assert_eq!(resolve(&ids_view, None, gone), Resolved::Unknown);

    // check: a retired ID that is current; a survivor that is not.
    let bad = redirects::encode(&[(live[5], None)]).unwrap();
    let survivor_missing = redirects::encode(&[(gone, Some(id(*b"np", 9_999)))]).unwrap();
    for (body, column) in [
        (bad, redirects::OLD_LO),
        (survivor_missing, redirects::NEW_LO),
    ] {
        let bytes = pack_of(&[(Kind::IDMAP, 500, &map), (Kind::REDIRECTS, 1, &body)]);
        let pack = Pack::open(&bytes).unwrap();
        let ids_view = IdMap::open(&pack.section(Kind::IDMAP).unwrap()).unwrap();
        let red_view = Redirects::open(&pack.section(Kind::REDIRECTS).unwrap()).unwrap();
        assert!(
            matches!(red_view.check(&ids_view), Err(Error::Payload { column: c, .. }) if c == column),
            "{column}"
        );
    }
}

#[test]
fn writers_and_checks_refuse_bad_input() {
    let a = id(*b"np", 1);
    assert!(matches!(
        idmap::encode(&[a, id(*b"np", 2), a]),
        Err(Error::Write(_))
    ));
    assert!(matches!(
        redirects::encode(&[(a, Some(a))]),
        Err(Error::Write(_))
    ));
    assert!(matches!(
        redirects::encode(&[(a, None), (a, Some(id(*b"np", 2)))]),
        Err(Error::Write(_))
    ));

    // Hand-built maps of rows [np/1, np/2]: sorted IDs out of order, an order that lies.
    let col = |v: &[u32]| packed(v, Coding::For);
    let (zero, lo, ns) = (col(&[0, 0]), col(&[1, 2]), a.get() >> 32);
    let hi = col(&[ns as u32, ns as u32]);
    let build = |sorted_lo: &[u32], order: &[u32]| {
        let (s, o) = (col(sorted_lo), col(order));
        table(&[
            (idmap::ID_HI, &hi),
            (idmap::ID_LO, &lo),
            (idmap::SORTED_HI, &hi),
            (idmap::SORTED_LO, &s),
            (idmap::ORDER, &o),
        ])
        .unwrap()
    };
    let problem = |body: &[u8]| {
        let p = pack_of(&[(Kind::IDMAP, 2, body)]);
        let pack = Pack::open(&p).unwrap();
        match IdMap::open(&pack.section(Kind::IDMAP).unwrap())
            .unwrap()
            .check()
        {
            Err(Error::Payload { column, .. }) => column,
            other => panic!("{other:?}"),
        }
    };
    open_map(&build(&[1, 2], &[0, 1]), 2, |m| m.check().unwrap());
    assert_eq!(problem(&build(&[2, 1], &[1, 0])), idmap::SORTED_LO);
    assert_eq!(problem(&build(&[1, 1], &[0, 0])), idmap::SORTED_LO);
    assert_eq!(problem(&build(&[1, 2], &[1, 0])), idmap::ORDER);
    // A zero ID (serial 0) in the rows.
    let zeros = table(&[
        (idmap::ID_HI, &zero),
        (idmap::ID_LO, &zero),
        (idmap::SORTED_HI, &zero),
        (idmap::SORTED_LO, &col(&[0, 1])),
        (idmap::ORDER, &col(&[0, 1])),
    ])
    .unwrap();
    assert_eq!(problem(&zeros), idmap::SORTED_LO);
    // A lying order never answers with the wrong row, even unchecked.
    open_map(&build(&[1, 2], &[1, 0]), 2, |m| {
        assert_eq!(m.row(a), None);
        assert_eq!(m.row(id(*b"np", 2)), None);
    });
}

#[test]
fn mutated_payloads_never_panic() {
    let v = ids(900, 4);
    let bodies = [
        (Kind::IDMAP, 900, idmap::encode(&v).unwrap()),
        (
            Kind::REDIRECTS,
            300,
            redirects::encode(
                &(0..300)
                    .map(|i| (id(*b"np", 10_000 + i), (i % 3 > 0).then(|| v[i as usize])))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ),
    ];
    let mut rng = Rng(5);
    for i in 0..4000 {
        let (kind, items, body) = &bodies[i % 2];
        let mut b = body.clone();
        for _ in 0..=rng.next() % 3 {
            let at = (rng.next() % b.len() as u64) as usize;
            b[at] = rng.next() as u8;
        }
        let p = pack_of(&[(*kind, *items, &b)]);
        let pack = Pack::open(&p).unwrap();
        let s = pack.section(*kind).unwrap();
        let probe = v[i % v.len()];
        if *kind == Kind::IDMAP {
            if let Ok(m) = IdMap::open(&s) {
                let _ = (m.check(), m.row(probe), m.id(i % 901));
            }
        } else if let Ok(r) = Redirects::open(&s) {
            let _ = (r.get(probe), r.get(id(*b"np", 10_000 + (i as u64 % 300))));
        }
    }
}
