//! The determinism gate (Phase 1, step 1.5): every kind of test pack, built twice from the same
//! inputs, is the same bytes; built from a changed input, it is not.
//!
//! Each build starts from fresh copies of the inputs and a fresh writer, and the second build
//! adds its sections and manifest entries in the reverse order of the first, so anything that
//! depends on call order, hash-map iteration order, the clock or an allocation shows up as a
//! byte difference.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; inputs are cut from a 64-bit generator"
)]

use ed25519_dalek::SigningKey;
use teigeodata_core::{E6, Namespace, PlaceId, PlaceKind, Point, hilbert};
use teigeodata_format::places::PlaceColumns;
use teigeodata_format::{
    Encoding, Kind, Pack, Tag, Writer, elevation, hierarchy, idmap, places, redirects, signer,
};

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

/// The inputs of every test pack, from one seed.
#[derive(Clone)]
struct Inputs {
    points: Vec<Point<E6>>,
    kinds: Vec<PlaceKind>,
    levels: Vec<u8>,
    classes: Vec<u32>,
    flags: Vec<u32>,
    elevations: Vec<Option<i16>>,
    parents: Vec<Option<u32>>,
    ids: Vec<PlaceId>,
    redirects: Vec<(PlaceId, Option<PlaceId>)>,
    build_id: u64,
    created: u64,
}

fn id(serial: u64) -> PlaceId {
    PlaceId::new(Namespace::from_country(*b"np").unwrap(), serial).unwrap()
}

fn inputs(n: usize, seed: u64) -> Inputs {
    let mut r = Rng(seed);
    let mut rows: Vec<(Point<E6>, PlaceKind, u8)> = (0..n)
        .map(|i| {
            let p = Point::<E6>::from_units(
                26_000_000 + r.below(4_000_000) as i32,
                80_000_000 + r.below(8_000_000) as i32,
            )
            .unwrap();
            match i {
                0 => (p, PlaceKind::Country, 0),
                1..=7 => (p, PlaceKind::Admin, 1),
                _ => (p, PlaceKind::Settlement, 0),
            }
        })
        .collect();
    rows.sort_by_key(|row| hilbert(row.0));
    // Parents point at an earlier row: always a forest, at most n deep, so keep it shallow by
    // choosing among the first few rows.
    let parents = (0..n)
        .map(|row| (row > 0).then(|| r.below(row.min(16) as u64) as u32))
        .collect();
    let ids: Vec<PlaceId> = (0..n as u64).map(|s| id(1 + s * 3 + r.below(3))).collect();
    // Retired IDs are the serials no current ID uses: 3 apart, offset past the last.
    let redirects = (0..n as u64 / 4)
        .map(|k| {
            let old = id(10_000_000 + k);
            let new = (k % 3 != 0).then(|| ids[r.below(n as u64) as usize]);
            (old, new)
        })
        .collect();
    Inputs {
        points: rows.iter().map(|row| row.0).collect(),
        kinds: rows.iter().map(|row| row.1).collect(),
        levels: rows.iter().map(|row| row.2).collect(),
        classes: (0..n).map(|_| r.below(5) as u32).collect(),
        flags: (0..n).map(|_| u32::from(r.below(4) == 0)).collect(),
        elevations: (0..n)
            // Row 0 always has a height, so every pack has an ELEVATION section.
            .map(|k| {
                (k == 0 || r.below(4) != 0).then(|| i16::try_from(r.below(9_000)).unwrap() - 100)
            })
            .collect(),
        parents,
        ids,
        redirects,
        build_id: r.next(),
        created: 1_762_473_600 + r.below(1_000_000),
    }
}

/// The section payloads of these inputs, in kind order.
fn payloads(i: &Inputs) -> Vec<(Kind, u64, Vec<u8>)> {
    let cols = PlaceColumns {
        points: &i.points,
        kinds: &i.kinds,
        admin_levels: &i.levels,
        classes: &i.classes,
        flags: &i.flags,
    };
    let n = i.points.len() as u64;
    let mut v = vec![
        (Kind::PLACES, n, places::encode(&cols).unwrap()),
        (
            Kind::ELEVATION,
            n,
            elevation::encode(&i.elevations).unwrap().unwrap(),
        ),
        (Kind::HIERARCHY, n, hierarchy::encode(&i.parents).unwrap()),
        (Kind::IDMAP, n, idmap::encode(&i.ids).unwrap()),
        (
            Kind::REDIRECTS,
            i.redirects.len() as u64,
            redirects::encode(&i.redirects).unwrap(),
        ),
    ];
    v.sort_by_key(|s| s.0);
    v
}

/// The packs under test: each payload alone, IDMAP with REDIRECTS, and every section in one
/// signed pack with manifest entries.
const PACKS: [(&str, &[Kind], bool); 7] = [
    ("places", &[Kind::PLACES], false),
    ("elevation", &[Kind::ELEVATION], false),
    ("hierarchy", &[Kind::HIERARCHY], false),
    ("idmap", &[Kind::IDMAP], false),
    ("redirects", &[Kind::IDMAP, Kind::REDIRECTS], false),
    (
        "all, signed",
        &[
            Kind::PLACES,
            Kind::HIERARCHY,
            Kind::IDMAP,
            Kind::REDIRECTS,
            Kind::ELEVATION,
        ],
        true,
    ),
    (
        "all, unsigned",
        &[
            Kind::PLACES,
            Kind::HIERARCHY,
            Kind::IDMAP,
            Kind::REDIRECTS,
            Kind::ELEVATION,
        ],
        false,
    ),
];

/// Builds one pack from a fresh copy of the inputs. `reverse` adds the sections and manifest
/// entries in the reverse order.
fn build(i: &Inputs, kinds: &[Kind], signed: bool, reverse: bool) -> Vec<u8> {
    let i = i.clone();
    let all = payloads(&i);
    let chosen: Vec<&(Kind, u64, Vec<u8>)> = all.iter().filter(|s| kinds.contains(&s.0)).collect();
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.build_id(i.build_id).created_unix(i.created);
    let schema = 1_u32.to_le_bytes();
    let mut entries: Vec<(Tag, &[u8])> = vec![
        (Tag::SCHEMA, &schema),
        (Tag::LICENCE, b"odbl"),
        (Tag(0x0100), b"an extension entry"),
    ];
    let mut order = chosen.clone();
    if reverse {
        entries.reverse();
        order.reverse();
    }
    for (tag, v) in entries {
        w.manifest(tag, v).unwrap();
    }
    for s in &order {
        w.section_bytes(s.0, Encoding::TABLE, s.1, &s.2).unwrap();
    }
    let mut layout = w.layout(signed).unwrap();
    if signed {
        let key = SigningKey::from_bytes(&[7; 32]);
        let block = signer(&key)(layout.digest());
        layout.set_signature(&block).unwrap();
    }
    // Bodies in the layout's order, whatever order they were added in.
    let bodies: Vec<&[u8]> = layout
        .placed()
        .iter()
        .map(|p| &chosen.iter().find(|s| s.0 == p.kind).unwrap().2[..])
        .collect();
    let bytes = layout.assemble(&bodies).unwrap();
    // A sanity check that the gate compares real packs.
    Pack::open_verified(&bytes).unwrap();
    bytes
}

#[test]
fn every_test_pack_builds_to_the_same_bytes() {
    for (n, seed) in [(1, 1), (9, 2), (300, 3), (2_000, 4)] {
        let i = inputs(n, seed);
        for (name, kinds, signed) in PACKS {
            let a = build(&i, kinds, signed, false);
            let b = build(&i, kinds, signed, true);
            assert!(a == b, "{name} pack of {n} rows: two builds differ");
        }
    }
}

#[test]
fn a_changed_input_changes_the_bytes() {
    let i = inputs(300, 3);
    let mut other = i.clone();
    // One field of one row at a time: the bytes of every pack holding it change.
    other.flags[17] ^= 1;
    let mut higher = i.clone();
    higher.elevations[42] = Some(higher.elevations[42].map_or(1, |e| e + 1));
    let mut moved = i.clone();
    moved.parents[200] = Some(0);
    let mut renumbered = i.clone();
    renumbered.ids[5] = id(9_000_000);
    let mut later = i.clone();
    later.created += 1;
    for (what, changed, affected) in [
        ("a flag", &other, &[Kind::PLACES][..]),
        ("an elevation", &higher, &[Kind::ELEVATION]),
        ("a parent", &moved, &[Kind::HIERARCHY]),
        ("an ID", &renumbered, &[Kind::IDMAP]),
        ("the creation time", &later, &[]),
    ] {
        for (name, kinds, signed) in PACKS {
            let same = build(&i, kinds, signed, false) == build(changed, kinds, signed, false);
            let touched = affected.is_empty() || kinds.iter().any(|k| affected.contains(k));
            assert_eq!(!same, touched, "{name} pack, changed {what}");
        }
    }
}
