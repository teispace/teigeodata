//! SUBSTRING (kind 23, D-199): word-suffix keys in the SEARCH layout. Prefix ranges and
//! best-first reads agree with brute force over the written keys; `check` proves each key's tag
//! and length, and refuses what SEARCH has but SUBSTRING must not (planted faults).
#![cfg(feature = "write")]
#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::panic,
    reason = "tests: inputs are cut from a 64-bit generator; a failure panics with its message"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use teigeodata_core::bitpack::encode_direct;
use teigeodata_core::table::{self, Codec};
use teigeodata_format::check;
use teigeodata_format::search::{self, EXACT, SUBSTRING, Search};
use teigeodata_format::{Encoding, Error, Kind, Pack, Writer};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

type Texts = BTreeMap<String, BTreeSet<u32>>;

/// Every suffix of 3 or more code points of names built from shared parts, Latin and
/// Devanagari, as the forge keys them (the rule itself is tested in `teigeodata-names`).
fn input(r: &mut Rng, rows: usize) -> (Texts, Vec<u8>) {
    const PARTS: [&str; 9] = [
        "kath",
        "mandu",
        "pur",
        "nagar",
        "pokhara",
        "काठ",
        "माडौं",
        "खरा",
        "बिराट",
    ];
    let mut keys = Texts::new();
    let importance: Vec<u8> = (0..rows).map(|_| r.next() as u8).collect();
    for row in 0..rows as u32 {
        let name: String = (0..=r.below(3))
            .map(|_| PARTS[r.below(PARTS.len())])
            .collect();
        let chars: Vec<char> = name.chars().collect();
        for start in 1..chars.len() {
            if chars.len() - start >= 3 {
                keys.entry(chars[start..].iter().collect())
                    .or_default()
                    .insert(row);
            }
        }
    }
    (keys, importance)
}

fn with_body(kind: Kind, body: &[u8], rows: u64) -> Vec<u8> {
    Writer::new("np", "substring", "2026.11.0")
        .unwrap()
        .pack(&[(kind, Encoding::TABLE, rows, body)], None)
        .unwrap()
}

fn problem(kind: Kind, body: &[u8], rows: u64) -> Option<(u32, &'static str)> {
    let file = with_body(kind, body, rows);
    let pack = Pack::open(&file).unwrap();
    match Search::open(&pack.section(kind).unwrap()).and_then(|s| s.check()) {
        Ok(()) => None,
        Err(Error::Payload { kind, problem, .. }) => Some((kind, problem)),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn ranges_and_best_reads_agree_with_brute_force() {
    let mut r = Rng(0x005e_ed23);
    for rows in [1, 7, 60, 400] {
        let (keys, importance) = input(&mut r, rows);
        let body = search::encode_substring(&keys, &importance).unwrap();
        // Deterministic.
        assert_eq!(body, search::encode_substring(&keys, &importance).unwrap());
        let file = with_body(Kind::SUBSTRING, &body, rows as u64);
        let pack = Pack::open(&file).unwrap();
        let s = Search::open(&pack.section(Kind::SUBSTRING).unwrap()).unwrap();
        s.check().unwrap();
        assert_eq!(s.kind(), Kind::SUBSTRING);
        assert_eq!(s.keys() as usize, keys.len());
        // The whole pack checks too.
        let mut checked = 0;
        check::sections(&pack, |k, res| {
            assert_eq!(k, Kind::SUBSTRING);
            assert!(res.unwrap());
            checked += 1;
        });
        assert_eq!(checked, 1);
        let texts: Vec<&String> = keys.keys().collect();
        for q in [
            "and",
            "mandu",
            "ur",
            "athm",
            "खरा",
            "माडौं",
            "ाडौं",
            "zzz",
            "agarp",
        ] {
            let range = s.range(SUBSTRING, q);
            let want: Vec<u32> = texts
                .iter()
                .enumerate()
                .filter(|(_, t)| t.starts_with(q))
                .map(|(i, _)| i as u32)
                .collect();
            assert_eq!(range.clone().collect::<Vec<_>>(), want, "{q}");
            // Nothing from another map.
            assert!(s.range(EXACT, q).is_empty());
            // Best-first: the keys whose best row beats a floor, as the engine's tier reads.
            for floor in [0_u8, 100, 200] {
                let mut got = Vec::new();
                s.best_in(
                    range.clone(),
                    || i32::from(floor),
                    |o| {
                        got.push(o);
                        ControlFlow::Continue(())
                    },
                );
                let best: Vec<u32> = want
                    .iter()
                    .copied()
                    .filter(|&o| {
                        keys[texts[o as usize]]
                            .iter()
                            .any(|&row| importance[row as usize] > floor)
                    })
                    .collect();
                assert_eq!(got, best, "{q} above {floor}");
            }
        }
    }
}

#[test]
fn the_writer_refuses_a_short_key() {
    let mut keys = Texts::new();
    keys.entry("ab".into()).or_default().insert(0);
    assert!(matches!(
        search::encode_substring(&keys, &[1]),
        Err(Error::Write(_))
    ));
    // Three code points that are six bytes are enough.
    let mut keys = Texts::new();
    keys.entry("काठ".into()).or_default().insert(0);
    assert!(search::encode_substring(&keys, &[1]).is_ok());
}

/// Raw keys (tag included), written with the SEARCH writer, which does not prove them.
fn raw(keys: &[&[u8]]) -> Vec<u8> {
    let mut m: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    for k in keys {
        m.entry(k.to_vec()).or_default().insert(0);
    }
    search::encode(&m, &[5]).unwrap()
}

#[test]
fn planted_faults_are_refused() {
    let sub = Kind::SUBSTRING;
    let good = raw(&[b"\x04abc", "\u{4}काठ".as_bytes()]);
    assert_eq!(problem(sub, &good, 1), None);
    let bad_key = Some((
        23,
        "a substring key is not its tag and 3 or more code points of UTF-8",
    ));
    // Two code points, the exact map's tag, a lone tag, and bytes that are not UTF-8.
    assert_eq!(problem(sub, &raw(&[b"\x04abc", b"\x04ab"]), 1), bad_key);
    assert_eq!(problem(sub, &raw(&[b"\x01abc"]), 1), bad_key);
    assert_eq!(problem(sub, &raw(&[b"\x04"]), 1), bad_key);
    assert_eq!(problem(sub, &raw(&[b"\x04ab\xff"]), 1), bad_key);
    // The same keys in SEARCH pass: its maps' rules are the forge's.
    assert_eq!(problem(Kind::SEARCH, &raw(&[b"\x04ab"]), 1), None);
    // SUBSTRING has no RULES or UNIT column.
    let t = table::Table::parse(&good).unwrap();
    let mut cols: Vec<(u16, Codec, Vec<u8>)> = Vec::new();
    for id in t.ids() {
        if id == search::AUTOMATON {
            cols.push((id, Codec::Bytes, t.bytes(id).unwrap().to_vec()));
        } else {
            let c = t.column(id).unwrap();
            let mut v = vec![0; c.len()];
            c.decode_all(&mut v);
            cols.push((id, Codec::Bitpack, encode_direct(&v)));
        }
    }
    cols.push((search::UNIT, Codec::Bitpack, encode_direct(&[0])));
    let refs: Vec<(u16, Codec, &[u8])> = cols.iter().map(|(i, c, b)| (*i, *c, &b[..])).collect();
    let with_unit = table::encode_with(&refs).unwrap();
    assert_eq!(problem(Kind::SEARCH, &with_unit, 1), None);
    assert_eq!(problem(sub, &with_unit, 1), Some((23, "an unknown column")));
}
