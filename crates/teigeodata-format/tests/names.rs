//! NAMES through the writer and the reader: a model round trip with per-script tables, every
//! writer refusal, every open and check rejection on hand-built tables, and no panic on mutated
//! payloads.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    reason = "tests: a failure panics with its message; inputs are cut from a 64-bit generator"
)]

use teigeodata_core::bitpack::{Coding, encode as packed};
use teigeodata_core::table::{Codec, Table, encode_with};
use teigeodata_format::names::{
    self, FIRST, HEAP, LANG, NameRecord, NameType, Names, SCRIPT, TABLE, TABLES, TEXT_BUFFER,
    TEXT_END, TYPE, Tag,
};
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

fn with_view<R>(rows: usize, body: &[u8], f: impl FnOnce(Result<Names<'_>, Error>) -> R) -> R {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.section_bytes(Kind::NAMES, Encoding::TABLE, rows as u64, body)
        .unwrap();
    let p = w.layout(false).unwrap().assemble(&[body]).unwrap();
    let pack = Pack::open(&p).unwrap();
    f(Names::open(&pack.section(Kind::NAMES).unwrap()))
}

/// Owned name records for `n` places: a Latin canonical name, then up to three more, some in
/// Devanagari, with every field set somewhere.
fn corpus(n: usize, seed: u64) -> Vec<Vec<Owned>> {
    let latin = [
        "Kath", "mandu", "Pokh", "ara", "Chowk", "Tole", "Bazar", "Khola", " ", "-",
    ];
    let deva = ["काठ", "माडौं", "पोखरा", "चोक", "टोल", "बजार", "खोला", " "];
    let mut r = Rng(seed);
    let word = |parts: &[&str], r: &mut Rng| -> String {
        (0..=r.next() % 3)
            .map(|_| parts[(r.next() % parts.len() as u64) as usize])
            .collect()
    };
    (0..n)
        .map(|i| {
            let mut v = vec![(
                word(&latin, &mut r),
                Some("ne"),
                Some("Latn"),
                Some(NameType::Official),
                None,
                None,
                1,
            )];
            for _ in 0..r.next() % 4 {
                let deva_name = r.next().is_multiple_of(2);
                v.push((
                    if deva_name {
                        word(&deva, &mut r)
                    } else {
                        word(&latin, &mut r)
                    },
                    if deva_name {
                        Some("ne")
                    } else {
                        [Some("en"), None, Some("mai")][i % 3]
                    },
                    if deva_name { Some("Deva") } else { None },
                    NameType::ALL.get((r.next() % 7) as usize).copied(),
                    (i % 5 == 0).then_some(-3000 + i32::try_from(i).unwrap()),
                    (i % 7 == 0).then_some(17_000 + i32::try_from(i).unwrap()),
                    0,
                ));
            }
            v
        })
        .collect()
}

type Owned = (
    String,
    Option<&'static str>,
    Option<&'static str>,
    Option<NameType>,
    Option<i32>,
    Option<i32>,
    u32,
);

fn records(places: &[Vec<Owned>]) -> Vec<Vec<NameRecord<'_>>> {
    places
        .iter()
        .map(|p| {
            p.iter()
                .map(|(t, l, s, k, f, to, rule)| NameRecord {
                    text: t,
                    lang: l.and_then(Tag::new),
                    script: s.and_then(Tag::new),
                    kind: *k,
                    valid_from: *f,
                    valid_to: *to,
                    rule: *rule,
                })
                .collect()
        })
        .collect()
}

fn encode(places: &[Vec<NameRecord<'_>>]) -> Result<Vec<u8>, Error> {
    let refs: Vec<&[NameRecord<'_>]> = places.iter().map(Vec::as_slice).collect();
    names::encode(&refs)
}

#[test]
fn round_trip() {
    for n in [1, 2, 129, 3000] {
        let owned = corpus(n, 7 + n as u64);
        let recs = records(&owned);
        let body = encode(&recs).unwrap();
        // 3000 places carry well over 16 KiB of Devanagari: a table of its own.
        let t = Table::parse(&body).unwrap();
        let tables = u32::from_le_bytes(t.bytes(TABLES).unwrap()[..4].try_into().unwrap());
        assert_eq!(tables, if n == 3000 { 2 } else { 1 }, "{n}");
        with_view(n, &body, |v| {
            let v = v.unwrap();
            v.check().unwrap();
            assert_eq!(v.len(), n);
            assert_eq!(v.records(), recs.iter().map(Vec::len).sum::<usize>());
            let mut buf = [0; TEXT_BUFFER];
            for (row, place) in recs.iter().enumerate() {
                let got: Vec<_> = v.names(row).collect();
                assert_eq!(got.len(), place.len());
                for (name, want) in got.iter().zip(place) {
                    assert_eq!(name.text(&mut buf).unwrap(), want.text);
                    assert_eq!(name.lang(), want.lang);
                    assert_eq!(name.script(), want.script);
                    assert_eq!(name.kind(), want.kind);
                    assert_eq!(name.valid(), (want.valid_from, want.valid_to));
                    assert_eq!(name.rule(), want.rule);
                }
                let c = v.canonical(row).unwrap();
                assert_eq!(c.text(&mut buf).unwrap(), place[0].text);
                assert_eq!(c.record(), v.of(row).start);
            }
            assert_eq!(v.of(n), 0..0);
            assert!(v.canonical(n).is_none());
            assert!(v.record(v.records()).is_none());
            // A buffer one byte short of the canonical name.
            let first = &place_text(&recs, 0);
            let mut short = vec![0; first.len().saturating_sub(1)];
            if !first.is_empty() {
                assert!(v.canonical(0).unwrap().text(&mut short).is_err());
            }
        });
    }
}

fn place_text(recs: &[Vec<NameRecord<'_>>], row: usize) -> String {
    recs[row][0].text.to_owned()
}

#[test]
fn optional_columns_are_left_out_when_unused() {
    let plain = [vec![NameRecord {
        text: "Lahachowk",
        ..NameRecord::default()
    }]];
    let body = encode(&plain).unwrap();
    let t = Table::parse(&body).unwrap();
    assert_eq!(
        t.ids().collect::<Vec<_>>(),
        [FIRST, TEXT_END, TABLE, TABLES, HEAP]
    );
    with_view(1, &body, |v| {
        let v = v.unwrap();
        let name = v.canonical(0).unwrap();
        assert_eq!(
            (
                name.lang(),
                name.script(),
                name.kind(),
                name.valid(),
                name.rule()
            ),
            (None, None, None, (None, None), 0)
        );
    });
}

#[test]
fn tags_and_types() {
    for code in ["ne", "mai", "Deva", "x1"] {
        let t = Tag::new(code).unwrap();
        assert_eq!(t.to_string(), code);
        assert_eq!(Tag::from_u32(t.get()), Some(t));
    }
    for bad in ["", "abcde", "n-e", "नेपा"] {
        assert_eq!(Tag::new(bad), None, "{bad}");
    }
    for bad in [0, 0x0000_2d61, 0x6100_0061] {
        assert_eq!(Tag::from_u32(bad), None, "{bad:#x}");
    }
    for (i, t) in NameType::ALL.into_iter().enumerate() {
        assert_eq!(t.code(), i as u32 + 1);
        assert_eq!(NameType::from_code(t.code()), Some(t));
        assert_ne!(t.as_str(), "");
    }
    assert_eq!(NameType::from_code(0), None);
    assert_eq!(NameType::from_code(7), None);
}

#[test]
fn the_writer_refuses_bad_input() {
    let ok = NameRecord {
        text: "Pokhara",
        ..NameRecord::default()
    };
    assert!(matches!(encode(&[vec![ok], vec![]]), Err(Error::Write(_))));
    let min_day = NameRecord {
        valid_to: Some(i32::MIN),
        ..ok
    };
    assert!(matches!(encode(&[vec![min_day]]), Err(Error::Write(_))));
    // Each code covers at most 8 bytes, so a text of 2,041 or more bytes needs more than 255.
    let long = "a".repeat(2041);
    let too_long = NameRecord { text: &long, ..ok };
    assert!(matches!(encode(&[vec![too_long]]), Err(Error::Write(_))));
}

/// Columns for hand-built tables, each replaceable.
struct Parts {
    first: Vec<u32>,
    text_end: Vec<u32>,
    table: Vec<u32>,
    tables: Vec<u8>,
    heap: Vec<u8>,
    extra: Vec<(u16, Codec, Vec<u8>)>,
}

/// Two places: "ab" and "c" (escaped bytes, a table of no symbols), then "d" for place 1.
fn parts() -> Parts {
    let mut tables = 1_u32.to_le_bytes().to_vec();
    tables.extend_from_slice(&[0; 8]);
    let mut heap = vec![255, b'a', 255, b'b', 255, b'c', 255, b'd'];
    heap.resize(8, 0);
    Parts {
        first: vec![0, 2, 3],
        text_end: vec![4, 6, 8],
        table: vec![0, 0, 0],
        tables,
        heap,
        extra: Vec::new(),
    }
}

fn verdict(rows: usize, p: &Parts) -> Option<(&'static str, u16)> {
    let col = |v: &[u32]| packed(v, Coding::For);
    let (first, end, table) = (col(&p.first), col(&p.text_end), col(&p.table));
    let mut cols: Vec<(u16, Codec, &[u8])> = vec![
        (FIRST, Codec::Bitpack, &first),
        (TEXT_END, Codec::Bitpack, &end),
        (TABLE, Codec::Bitpack, &table),
    ];
    for (id, c, b) in &p.extra {
        cols.push((*id, *c, b));
    }
    cols.push((TABLES, Codec::Bytes, &p.tables));
    cols.push((HEAP, Codec::Bytes, &p.heap));
    cols.sort_by_key(|c| c.0);
    let body = encode_with(&cols).unwrap();
    with_view(rows, &body, |v| match v {
        Ok(v) => match v.check() {
            Ok(()) => None,
            Err(Error::Payload { column, .. }) => Some(("check", column)),
            Err(e) => panic!("{e}"),
        },
        Err(Error::Payload { column, .. }) => Some(("open", column)),
        Err(e) => panic!("{e}"),
    })
}

/// Hand-built tables the writer never makes, each with the row count and the verdict: these
/// fail at open.
fn bad_frames() -> Vec<(Parts, usize, (&'static str, u16))> {
    let with = |f: fn(&mut Parts)| {
        let mut p = parts();
        f(&mut p);
        p
    };
    vec![
        // Lengths.
        (parts(), 3, ("open", FIRST)),
        (with(|p| p.table = vec![0, 0]), 2, ("open", TABLE)),
        (
            with(|p| {
                p.extra
                    .push((LANG, Codec::Bitpack, packed(&[0], Coding::For)));
            }),
            2,
            ("open", LANG),
        ),
        (
            with(|p| {
                p.extra
                    .push((12, Codec::Bitpack, packed(&[0, 0, 0], Coding::For)));
            }),
            2,
            ("open", 0),
        ),
        (
            with(|p| {
                p.extra
                    .push((0, Codec::Bitpack, packed(&[0, 0, 0], Coding::For)));
            }),
            2,
            ("open", 0),
        ),
        // Tables.
        (with(|p| p.tables[0] = 0), 2, ("open", TABLES)),
        (with(|p| p.tables = vec![0; 4]), 2, ("open", TABLES)),
        (with(|p| p.tables[0] = 17), 2, ("open", TABLES)),
        (with(|p| p.tables[0] = 2), 2, ("open", TABLES)),
        (
            with(|p| p.tables.extend_from_slice(&[0; 4])),
            2,
            ("open", TABLES),
        ),
        (with(|p| p.tables[5] = 1), 2, ("open", TABLES)),
    ]
}

/// Tables that open but fail the deep check.
fn bad_values() -> Vec<(Parts, usize, (&'static str, u16))> {
    let with = |f: fn(&mut Parts)| {
        let mut p = parts();
        f(&mut p);
        p
    };
    vec![
        // Places.
        (with(|p| p.first = vec![1, 2, 3]), 2, ("check", FIRST)),
        (with(|p| p.first = vec![0, 0, 3]), 2, ("check", FIRST)),
        (with(|p| p.first = vec![0, 2, 2]), 2, ("check", FIRST)),
        (with(|p| p.first = vec![0, 1, 2]), 2, ("check", FIRST)),
        // A record of 256 codes (128 escaped bytes).
        (
            with(|p| {
                p.heap = [255, b'a'].repeat(128);
                p.heap.extend_from_slice(&[255, b'c', 255, b'd']);
                p.text_end = vec![256, 258, 260];
            }),
            2,
            ("check", HEAP),
        ),
        // Text spans and the heap.
        (with(|p| p.text_end = vec![4, 2, 8]), 2, ("check", TEXT_END)),
        (
            with(|p| p.text_end = vec![4, 6, 12]),
            2,
            ("check", TEXT_END),
        ),
        (
            with(|p| p.heap.extend_from_slice(&[0; 4])),
            2,
            ("check", HEAP),
        ),
        (
            with(|p| {
                p.heap = vec![255, b'a', 255, b'b', 255, b'c', 255, b'd', 0, 0, 0, 0];
                p.text_end = vec![4, 6, 9];
            }),
            2,
            ("check", HEAP),
        ),
        (
            with(|p| {
                p.heap = vec![255, b'a', 255, b'b', 255, b'c', 255, b'd', 7, 0, 0, 0];
            }),
            2,
            ("check", HEAP),
        ),
        // A lone escape; a code with no symbol; bytes that are not UTF-8.
        (with(|p| p.text_end = vec![3, 6, 8]), 2, ("check", HEAP)),
        (with(|p| p.heap[0] = 3), 2, ("check", HEAP)),
        (with(|p| p.heap[1] = 0xff), 2, ("check", HEAP)),
        // Values.
        (with(|p| p.table = vec![0, 1, 0]), 2, ("check", TABLE)),
        (
            with(|p| {
                p.extra
                    .push((TYPE, Codec::Bitpack, packed(&[1, 7, 0], Coding::For)));
            }),
            2,
            ("check", TYPE),
        ),
        (
            with(|p| {
                p.extra
                    .push((SCRIPT, Codec::Bitpack, packed(&[0, 0, 0x2d61], Coding::For)));
            }),
            2,
            ("check", SCRIPT),
        ),
        (
            with(|p| {
                p.extra.push((
                    LANG,
                    Codec::Bitpack,
                    packed(&[0x6100_0061, 0, 0], Coding::For),
                ));
            }),
            2,
            ("check", LANG),
        ),
    ]
}

#[test]
fn open_and_check_refuse_bad_tables() {
    assert_eq!(verdict(2, &parts()), None);
    let cases = bad_frames()
        .into_iter()
        .chain(bad_values())
        .collect::<Vec<_>>();
    for (i, (p, rows, want)) in cases.iter().enumerate() {
        assert_eq!(verdict(*rows, p), Some(*want), "case {i}");
    }
}

#[test]
fn mutated_payloads_never_panic() {
    let owned = corpus(400, 3);
    let recs = records(&owned);
    let body = encode(&recs).unwrap();
    let mut rng = Rng(9);
    let mut buf = [0; TEXT_BUFFER];
    for i in 0..3000 {
        let mut b = body.clone();
        for _ in 0..=rng.next() % 3 {
            let at = (rng.next() % b.len() as u64) as usize;
            b[at] = rng.next() as u8;
        }
        with_view(400, &b, |v| {
            if let Ok(v) = v {
                let _ = v.check();
                for name in v.names(i % 401).take(300) {
                    let _ = (
                        name.text(&mut buf).is_ok(),
                        name.lang(),
                        name.kind(),
                        name.valid(),
                    );
                }
                let _ = v.record(i).map(|n| n.text(&mut buf[..i % 40]).is_ok());
            }
        });
    }
}

/// D-227: every origin code round-trips, an unknown one reads as `Other`, and only the codes
/// a program writes are machine output.
#[test]
fn origins_round_trip_and_say_which_are_machine() {
    use teigeodata_format::names::Origin;
    for code in 0..=10 {
        let o = Origin::from_code(code);
        assert_eq!(o.code(), code);
        assert!(!matches!(o, Origin::Other(_)), "{code}");
        assert_eq!(o.is_machine(), (7..=9).contains(&code), "{code}");
    }
    assert_eq!(Origin::from_code(11), Origin::Other(11));
    assert!(!Origin::from_code(11).is_machine());
    assert_eq!(Origin::default(), Origin::Unknown);
}
