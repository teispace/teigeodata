//! POSTAL against its input: lookups in many input forms, prefix search, code text, records,
//! validity, redirects and place → codes against brute force; then the refusals of the writer,
//! of `check`, and hostile bytes.
#![cfg(feature = "write")]
#![allow(
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::panic,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::assert_is_empty,
    reason = "tests: inputs are cut from a 64-bit generator; a failure panics with its message"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use teigeodata_format::postal::{self, Code, CodeKind, Postal, Rule};
use teigeodata_format::search::fst::MAX_KEY;
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

fn rule(name: &str, templates: &[&str]) -> Rule {
    Rule {
        name: name.into(),
        templates: templates.iter().map(|t| (*t).into()).collect(),
        ..Rule::default()
    }
}

/// Nepal's two schemes (D-090), GB, JP, LV and FR: every case of the scheme table.
fn rules() -> Vec<Rule> {
    vec![
        Rule {
            admin_aligned: true,
            ignore: " -".into(),
            valid_from: Some(20_339),
            ..rule("np", &["99999", "9999999"])
        },
        Rule {
            ignore: " -".into(),
            valid_to: Some(20_338),
            ..rule("np-1991", &["99999"])
        },
        Rule {
            upper: true,
            ignore: " ".into(),
            ..rule(
                "gb",
                &[
                    "A9 9AA", "A99 9AA", "AA9 9AA", "AA99 9AA", "A9A 9AA", "AA9A 9AA",
                ],
            )
        },
        Rule {
            ignore: " -".into(),
            ..rule("jp", &["999-9999"])
        },
        Rule {
            upper: true,
            ignore: " -".into(),
            prefixes: vec!["LV-".into()],
            display: "LV-".into(),
            ..rule("lv", &["9999"])
        },
        Rule {
            upper: true,
            ignore: " ".into(),
            ..rule(
                "fr",
                &["99999", "99999 \\C\\E\\D\\E\\X", "99999 \\C\\E\\D\\E\\X 99"],
            )
        },
    ]
}

fn code(scheme: &str, code: &str, place: u32) -> Code {
    Code {
        scheme: scheme.into(),
        code: code.into(),
        place,
        kind: CodeKind::Area,
        valid_from: None,
        valid_to: None,
        redirect: None,
    }
}

fn with_body(body: &[u8], records: u64) -> Vec<u8> {
    Writer::new("np", "postal", "2026.11.0")
        .unwrap()
        .pack(&[(Kind::POSTAL, Encoding::TABLE, records, body)], None)
        .unwrap()
}

fn open(bytes: &[u8], places: usize) -> Result<Postal<'_>, Error> {
    let pack = Pack::open(bytes)?;
    Postal::open(&pack.section(Kind::POSTAL).unwrap(), places)
}

/// Every (scheme, code) that `input` finds.
fn found(p: &Postal<'_>, input: &str, scheme: Option<&str>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut buf = [0_u8; MAX_KEY];
    p.lookup(input, scheme, |o| {
        let (s, c) = p.code(o, &mut buf).unwrap();
        out.push((s.to_string(), c.to_string()));
        ControlFlow::Continue(())
    });
    out
}

fn pair(s: &str, c: &str) -> (String, String) {
    (s.into(), c.into())
}

fn sample() -> (Vec<u8>, usize) {
    let mut codes = vec![
        code("np", "30608", 3),
        Code {
            kind: CodeKind::Part,
            ..code("np", "3060801", 3)
        },
        Code {
            kind: CodeKind::Part,
            ..code("np", "3060832", 3)
        },
        code("np", "10101", 1),
        Code {
            redirect: Some(("np".into(), "30608".into())),
            kind: CodeKind::PostOffice,
            ..code("np-1991", "44600", 3)
        },
        // The same string in both schemes, with different places (67 such in Nepal).
        code("np-1991", "10101", 2),
        code("gb", "SW1A 1AA", 4),
        code("gb", "B1 1AA", 5),
        code("gb", "b11 1aa", 6),
        code("jp", "1000001", 7),
        code("lv", "LV-1050", 8),
        code("fr", "75021 cedex 01", 9),
        code("fr", "75001", 9),
    ];
    codes.push(Code {
        valid_to: Some(20_000),
        ..code("np", "30608", 0)
    });
    let body = postal::encode(&rules(), &codes, 10, true).unwrap();
    (with_body(&body, codes.len() as u64), 10)
}

#[test]
fn every_input_form_finds_its_code() {
    let (bytes, places) = sample();
    let p = open(&bytes, places).unwrap();
    p.check().unwrap();
    assert_eq!(p.keys(), 13);
    // Separators, case, full-width, Devanagari digits, a prefix and its absence.
    assert_eq!(found(&p, "30608", Some("np")), [pair("np", "30608")]);
    assert_eq!(found(&p, " 306-08 ", Some("np")), [pair("np", "30608")]);
    assert_eq!(found(&p, "३०६०८", Some("np")), [pair("np", "30608")]);
    assert_eq!(found(&p, "３０６０８０１", None), [pair("np", "3060801")]);
    assert_eq!(found(&p, "sw1a1aa", None), [pair("gb", "SW1A 1AA")]);
    assert_eq!(found(&p, "B111AA", None), [pair("gb", "B11 1AA")]);
    assert_eq!(found(&p, "B11AA", None), [pair("gb", "B1 1AA")]);
    assert_eq!(found(&p, "1000001", Some("jp")), [pair("jp", "100-0001")]);
    assert_eq!(found(&p, "100–0001", Some("jp")), [pair("jp", "100-0001")]);
    assert_eq!(found(&p, "lv-1050", None), [pair("lv", "1050")]);
    assert_eq!(found(&p, "1050", Some("lv")), [pair("lv", "1050")]);
    assert_eq!(
        found(&p, "75021 Cedex 01", None),
        [pair("fr", "75021 CEDEX 01")]
    );
    // The same string in two schemes: both, each with its scheme.
    assert_eq!(
        found(&p, "10101", None),
        [pair("np", "10101"), pair("np-1991", "10101")]
    );
    // Not found, never an error: a wrong shape, letters in a digit scheme, other scripts.
    for bad in [
        "3060",
        "306080",
        "3060a",
        "",
        "   ",
        "ab",
        "काठमाडौं",
        "30608\u{0}",
        "SW1A 1A",
    ] {
        assert!(found(&p, bad, Some("np")).is_empty(), "{bad:?}");
        assert!(found(&p, bad, Some("gb")).is_empty(), "{bad:?}");
    }
    assert!(found(&p, "30608", Some("xx")).is_empty());
}

#[test]
fn prefixes_records_and_links() {
    let (bytes, places) = sample();
    let p = open(&bytes, places).unwrap();
    let mut buf = [0_u8; MAX_KEY];
    let list = |input: &str, scheme: Option<&str>| {
        let mut out = Vec::new();
        let mut buf = [0_u8; MAX_KEY];
        p.prefix(input, scheme, |o| {
            out.push(p.code(o, &mut buf).unwrap().1.to_string());
            ControlFlow::Continue(())
        });
        out
    };
    assert_eq!(list("306", Some("np")), ["30608", "3060801", "3060832"]);
    assert_eq!(list("३०६०८०", Some("np")), ["3060801"]);
    assert_eq!(list("३०६०८", Some("np")), ["30608", "3060801", "3060832"]);
    // GB: "B1" covers B1 and B11; "B11" only B11 1AA ("B1 1…" with the space).
    assert_eq!(list("b1", Some("gb")), ["B1 1AA", "B11 1AA"]);
    assert_eq!(list("B11", Some("gb")), ["B1 1AA", "B11 1AA"]);
    assert_eq!(list("B111", Some("gb")), ["B11 1AA"]);
    assert_eq!(list("100", Some("jp")), ["100-0001"]);
    assert!(list("", None).is_empty());
    assert!(list("9", None).is_empty());
    // Stopping early.
    let mut n = 0;
    p.prefix("306", None, |_| {
        n += 1;
        ControlFlow::Break(())
    });
    assert_eq!(n, 1);

    // Records: kinds, places in order, the scheme's validity, a record's own date.
    let o = p.lookup_one("30608");
    let recs: Vec<_> = p.records_of(o).map(|r| p.record(r).unwrap()).collect();
    assert_eq!(recs.len(), 2);
    assert_eq!((recs[0].place, recs[0].valid_to), (0, Some(20_000)));
    assert_eq!((recs[1].place, recs[1].valid_from), (3, Some(20_339)));
    assert_eq!(recs[1].kind, CodeKind::Area);
    let old = p.records_of(found_one(&p, "44600", "np-1991")).start;
    let rec = p.record(old).unwrap();
    assert_eq!(rec.kind, CodeKind::PostOffice);
    assert_eq!(rec.valid_to, Some(20_338));
    assert_eq!(
        p.code(rec.redirect.unwrap(), &mut buf),
        Some(("np", "30608"))
    );
    for r in 0..p.records() {
        assert!(p.records_of(p.key_of(r).unwrap()).contains(&r));
    }
    assert_eq!(p.key_of(p.records()), None);

    // Place → codes.
    let mut at3 = Vec::new();
    p.records_at(3, |r| {
        let (s, c) = p.code(p.key_of(r).unwrap(), &mut buf).unwrap();
        at3.push(format!("{s}:{c}"));
        ControlFlow::Continue(())
    });
    assert_eq!(
        at3,
        ["np:30608", "np:3060801", "np:3060832", "np-1991:44600"]
    );
    let mut none = 0;
    p.records_at(99, |_| {
        none += 1;
        ControlFlow::Continue(())
    });
    assert_eq!(none, 0);
    let s = p.scheme("lv").unwrap();
    assert_eq!(s.display_prefix(), "LV-");
    assert!(p.scheme("np").unwrap().admin_aligned());
    assert_eq!(
        p.schemes()
            .into_iter()
            .map(|s| s.name())
            .collect::<Vec<_>>(),
        ["fr", "gb", "jp", "lv", "np", "np-1991"]
    );
}

fn found_one(p: &Postal<'_>, input: &str, scheme: &str) -> u32 {
    let mut out = None;
    p.lookup(input, Some(scheme), |o| {
        out = Some(o);
        ControlFlow::Break(())
    });
    out.unwrap()
}

trait One {
    fn lookup_one(&self, input: &str) -> u32;
}

impl One for Postal<'_> {
    fn lookup_one(&self, input: &str) -> u32 {
        found_one(self, input, "np")
    }
}

/// Random codes over many places: lookups, display round trips, prefix ranges and place →
/// codes against brute force, across several 128-value blocks.
#[test]
fn random_codes_agree_with_brute_force() {
    let mut r = Rng(17);
    let places = 3000_u32;
    let mut codes = Vec::new();
    let mut want: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
    for _ in 0..if cfg!(miri) { 200 } else { 6000 } {
        let (scheme, text) = if r.below(2) == 0 {
            ("np", format!("{:05}", 10_000 + r.below(800)))
        } else {
            let l = |r: &mut Rng| char::from(b'A' + r.below(26) as u8);
            let d = |r: &mut Rng| char::from(b'0' + r.below(10) as u8);
            (
                "gb",
                format!(
                    "{}{}{} {}{}{}",
                    l(&mut r),
                    d(&mut r),
                    d(&mut r),
                    d(&mut r),
                    l(&mut r),
                    l(&mut r)
                ),
            )
        };
        let place = r.below(places as usize) as u32;
        if want
            .entry((scheme.into(), text.clone()))
            .or_default()
            .insert(place)
        {
            codes.push(code(scheme, &text, place));
        }
    }
    let body = postal::encode(&rules(), &codes, places, true).unwrap();
    let bytes = with_body(&body, codes.len() as u64);
    let p = open(&bytes, places as usize).unwrap();
    p.check().unwrap();
    assert_eq!(p.keys() as usize, want.len());
    let mut buf = [0_u8; MAX_KEY];
    for (o, ((scheme, text), rows)) in want.iter().enumerate() {
        let o = o as u32;
        assert_eq!(p.code(o, &mut buf), Some((scheme.as_str(), text.as_str())));
        // The canonical code, and its display without separators, find the key again.
        assert_eq!(found(&p, text, Some(scheme)), [pair(scheme, text)]);
        assert_eq!(
            found(&p, &text.replace(' ', "").to_lowercase(), Some(scheme)),
            [pair(scheme, text)]
        );
        let got: BTreeSet<u32> = p
            .records_of(o)
            .map(|rec| p.record(rec).unwrap().place)
            .collect();
        assert_eq!(&got, rows);
    }
    // Prefix ranges.
    for probe in ["1", "10", "105", "A", "A1", "Q99", "Z9"] {
        let mut got = Vec::new();
        p.prefix(probe, None, |o| {
            got.push(o);
            ControlFlow::Continue(())
        });
        let mut expect: Vec<u32> = Vec::new();
        for (o, ((_, text), _)) in want.iter().enumerate() {
            if text.starts_with(probe) {
                expect.push(o as u32);
            }
        }
        assert_eq!(got, expect, "{probe}");
    }
    // Place → codes.
    let mut by_place: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for rec in 0..p.records() {
        by_place
            .entry(p.record(rec).unwrap().place)
            .or_default()
            .push(rec);
    }
    for place in 0..places {
        let mut got = Vec::new();
        p.records_at(place, |rec| {
            got.push(rec);
            ControlFlow::Continue(())
        });
        assert_eq!(got, by_place.get(&place).cloned().unwrap_or_default());
    }
}

#[test]
fn the_writer_refuses_bad_input() {
    let w = |codes: &[Code]| postal::encode(&rules(), codes, 10, false).map(|_| ());
    assert!(w(&[code("np", "3060", 1)]).is_err(), "no template");
    assert!(w(&[code("xx", "30608", 1)]).is_err(), "no scheme");
    assert!(w(&[code("np", "30608", 10)]).is_err(), "row past the end");
    assert!(
        w(&[code("np", "30608", 1), code("np", "306 08", 1)]).is_err(),
        "the same place twice"
    );
    assert!(
        w(&[Code {
            redirect: Some(("np".into(), "99999".into())),
            ..code("np", "30608", 1)
        }])
        .is_err(),
        "a redirect to a missing code"
    );
    let bad_rules = [
        rule("NP", &["99999"]),
        rule("np", &[]),
        rule("np", &["\\"]),
        rule("np", &["-"]),
        Rule {
            valid_from: Some(5),
            valid_to: Some(4),
            ..rule("np", &["99999"])
        },
    ];
    for r in bad_rules {
        assert!(
            postal::encode(std::slice::from_ref(&r), &[], 1, false).is_err(),
            "{r:?}"
        );
    }
    assert!(postal::encode(&[rule("np", &["9"]), rule("np", &["9"])], &[], 1, false).is_err());
    // The same input gives the same bytes.
    let codes = [code("np", "30608", 1), code("gb", "SW1A1AA", 2)];
    assert_eq!(
        postal::encode(&rules(), &codes, 10, true).unwrap(),
        postal::encode(&rules(), &codes, 10, true).unwrap()
    );
}

#[test]
fn check_refuses_and_hostile_bytes_never_panic() {
    let (bytes, places) = sample();
    // Fewer places than a record names.
    let p = open(&bytes, 4).unwrap();
    assert!(p.check().is_err());
    let mut r = Rng(9);
    let cases = if cfg!(miri) { 50 } else { 20_000 };
    let mut refused = 0;
    let section = Pack::open(&bytes).unwrap().section(Kind::POSTAL).unwrap();
    let body = section.bytes().to_vec();
    for _ in 0..cases {
        let mut b = body.clone();
        for _ in 0..=r.below(3) {
            let i = r.below(b.len());
            b[i] = r.next() as u8;
        }
        let packed = with_body(&b, 14);
        let Ok(p) = open(&packed, places) else {
            refused += 1;
            continue;
        };
        if p.check().is_err() {
            refused += 1;
        }
        let mut buf = [0_u8; MAX_KEY];
        p.lookup("30608", None, |_| ControlFlow::Continue(()));
        p.prefix("3", None, |_| ControlFlow::Continue(()));
        p.records_at(3, |_| ControlFlow::Continue(()));
        for o in 0..p.keys().min(20) {
            let _ = p.code(o, &mut buf);
            for rec in p.records_of(o) {
                let _ = p.record(rec);
                let _ = p.key_of(rec);
            }
        }
    }
    assert!(refused > cases / 2, "{refused}");
}

/// A body whose SCHEMES come from another rule: its keys are no longer canonical, and only
/// the canonical check sees it.
#[test]
fn check_refuses_a_key_that_is_not_canonical() {
    use teigeodata_core::table::{self, Table};
    let codes = [code("np", "30608", 0)];
    let spaced = Rule {
        ignore: " ".into(),
        ..rule("np", &["999 99"])
    };
    let a = postal::encode(&[rule("np", &["99999"])], &codes, 1, false).unwrap();
    let b = postal::encode(&[spaced], &codes, 1, false).unwrap();
    let (ta, tb) = (Table::parse(&a).unwrap(), Table::parse(&b).unwrap());
    let cols: Vec<_> = ta
        .ids()
        .map(|id| {
            let from = if id == postal::SCHEMES { &tb } else { &ta };
            let (codec, bytes) = from.column_raw(id).unwrap();
            (id, codec, bytes)
        })
        .collect();
    let spliced = table::encode_with(&cols).unwrap();
    let ok = with_body(&a, 1);
    open(&ok, 1).unwrap().check().unwrap();
    let bytes = with_body(&spliced, 1);
    let p = open(&bytes, 1).unwrap();
    assert!(p.check().is_err());
}

/// Units with wards, as in Nepal: long runs of one place, codes of a unit split across runs
/// by other schemes' keys. Places, place → codes and runs against brute force.
#[test]
fn ward_runs_agree_with_brute_force() {
    let mut r = Rng(23);
    let places = 900_u32;
    let mut codes = Vec::new();
    for unit in 0..if cfg!(miri) { 20 } else { 700 } {
        let place = r.below(places as usize) as u32;
        let local = format!("{}", 10_000 + unit * 7);
        codes.push(code("np", &local, place));
        for w in 1..=1 + r.below(30) {
            codes.push(Code {
                kind: CodeKind::Part,
                ..code("np", &format!("{local}{w:02}"), place)
            });
        }
        if r.below(4) == 0 {
            codes.push(code("np-1991", &format!("{}", 40_000 + unit), place));
        }
    }
    let body = postal::encode(&rules(), &codes, places, true).unwrap();
    let bytes = with_body(&body, codes.len() as u64);
    let p = open(&bytes, places as usize).unwrap();
    p.check().unwrap();
    let mut buf = [0_u8; MAX_KEY];
    let mut want: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for c in &codes {
        want.entry(c.place)
            .or_default()
            .insert(format!("{}:{}", c.scheme, c.code));
        let o = found_one(&p, &c.code, &c.scheme);
        let rec = p.records_of(o).find(|&x| p.place_of(x) == Some(c.place));
        assert_eq!(p.record(rec.unwrap()).unwrap().kind, c.kind, "{}", c.code);
    }
    for place in 0..places {
        let mut got = BTreeSet::new();
        let mut last = None;
        p.records_at(place, |rec| {
            assert!(last.is_none_or(|l| rec > l), "records out of order");
            last = Some(rec);
            let (s, c) = p.code(p.key_of(rec).unwrap(), &mut buf).unwrap();
            got.insert(format!("{s}:{c}"));
            ControlFlow::Continue(())
        });
        assert_eq!(
            got,
            want.get(&place).cloned().unwrap_or_default(),
            "place {place}"
        );
    }
}
