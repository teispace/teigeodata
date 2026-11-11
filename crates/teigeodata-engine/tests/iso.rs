//! ISO 3166 codes (D-197): `iso_code` gives a country's and a unit's code from the first layer
//! that holds the place with one, so a part's stub carries its unit's code and a pack without
//! codes falls through; `by_iso` finds the place; slices and merges keep the codes.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "tests: failures panic with their message; values are small"
)]

mod common;

use common::{Model, synthetic};
use teigeodata_core::{IsoCode, PlaceKind};
use teigeodata_engine::{Component, Components, Engine, Error, Layer, LayerOptions};
use teigeodata_format::extract;
use teigeodata_format::region::Options;
use teigeodata_format::{Encoding, Kind, Pack, Writer};

fn code(s: &str) -> IsoCode {
    s.parse().unwrap()
}

/// The synthetic country with codes: `NP` on the country, `NP-D1` … on its level-1 districts.
fn coded() -> Model {
    let mut m = synthetic(300, 7);
    let mut d = 0;
    for (r, (&kind, &level)) in m.kinds.iter().zip(&m.levels).enumerate() {
        if kind == PlaceKind::Country {
            m.iso.push((r as u32, code("NP")));
        } else if kind == PlaceKind::Admin && level == 1 {
            d += 1;
            m.iso.push((r as u32, code(&format!("NP-D{d}"))));
        }
    }
    assert_eq!(m.iso.len(), 9, "the country and 8 districts");
    m
}

fn engine(packs: &[(Vec<u8>, i32)]) -> Engine {
    let mut e = Engine::new();
    for (p, priority) in packs {
        let o = LayerOptions {
            priority: *priority,
            ..LayerOptions::default()
        };
        e.add(Layer::from_bytes(p.clone(), o).unwrap());
    }
    e
}

#[test]
fn a_country_and_its_units_have_their_codes() {
    let m = coded();
    let e = engine(&[(m.encode(), 0)]);
    for &(row, c) in &m.iso {
        let p = e.get(m.ids[row as usize]).unwrap().unwrap();
        assert_eq!(e.iso_code(&p).unwrap(), Some(c));
        assert_eq!(e.by_iso(c).unwrap().map(|q| q.id), Some(p.id));
        assert_eq!(c.is_country(), p.kind == Some(PlaceKind::Country));
    }
    // A settlement has none; its chain gives its district's code, then the country's.
    let s = m
        .kinds
        .iter()
        .zip(&m.parents)
        .position(|(&k, p)| k == PlaceKind::Settlement && p.is_some())
        .unwrap();
    let p = e.get(m.ids[s]).unwrap().unwrap();
    assert_eq!(e.iso_code(&p).unwrap(), None);
    let mut chain = Vec::new();
    e.ancestors(&p, &mut chain).unwrap();
    let codes: Vec<String> = chain
        .iter()
        .filter_map(|a| e.iso_code(a).unwrap())
        .map(|c| c.as_str().to_owned())
        .collect();
    assert_eq!(codes.len(), 2, "{codes:?}");
    assert!(
        codes[0].starts_with("NP-D") && codes[1] == "NP",
        "{codes:?}"
    );
    assert_eq!(e.by_iso(code("IN-TN")).unwrap(), None);
}

#[test]
fn a_stub_carries_its_code_and_a_pack_without_codes_falls_through() {
    let m = coded();
    let whole = m.encode();
    // Parts by row parity hold the districts and the country as stubs, with their codes.
    let parts = m.split(2, |r, _| (r % 2) as usize);
    for part in &parts {
        let pm = Model::from_pack(part);
        let want: Vec<(u64, IsoCode)> = m
            .iso
            .iter()
            .filter(|(w, _)| pm.ids.contains(&m.ids[*w as usize]))
            .map(|&(w, c)| (m.ids[w as usize].get(), c))
            .collect();
        let mut got: Vec<(u64, IsoCode)> = pm
            .iso
            .iter()
            .map(|&(r, c)| (pm.ids[r as usize].get(), c))
            .collect();
        got.sort_unstable();
        let mut want = want;
        want.sort_unstable();
        assert_eq!(got, want, "every coded row, stubs included, keeps its code");
        assert!(got.iter().any(|&(_, c)| c.is_country()), "the country stub");
    }
    // A newer pack of the same places without codes, above the coded whole pack.
    let mut bare = m.clone();
    bare.iso.clear();
    bare.address = "np-bare".into();
    let mut packs: Vec<(Vec<u8>, i32)> = parts.into_iter().map(|p| (p, 1)).collect();
    packs.push((bare.encode(), 2));
    packs.push((whole, 0));
    let e = engine(&packs);
    for &(row, c) in &m.iso {
        let p = e.get(m.ids[row as usize]).unwrap().unwrap();
        assert_eq!(e.iso_code(&p).unwrap(), Some(c), "{c}");
        // The place by its code is the winning layer's (the bare pack's) answer.
        let found = e.by_iso(c).unwrap().unwrap();
        assert_eq!((found.id, found.layer), (p.id, p.layer));
    }
}

#[test]
fn codes_need_the_admin_component() {
    let m = coded();
    let mut e = Engine::new();
    let o = LayerOptions {
        installed: Some(Components::of(&[Component::Point])),
        ..LayerOptions::default()
    };
    e.add(Layer::from_bytes(m.encode(), o).unwrap());
    let p = e.get(m.ids[m.iso[0].0 as usize]).unwrap().unwrap();
    assert!(matches!(
        e.iso_code(&p),
        Err(Error::MissingComponent {
            component: Component::Admin,
            ..
        })
    ));
    assert!(e.by_iso(m.iso[0].1).is_err());
    // A pack without codes answers none, with or without the component.
    let mut bare = m.clone();
    bare.iso.clear();
    let e = engine(&[(bare.encode(), 0)]);
    let p = e.get(m.ids[m.iso[0].0 as usize]).unwrap().unwrap();
    assert_eq!(e.iso_code(&p).unwrap(), None);
    assert_eq!(e.by_iso(m.iso[0].1).unwrap(), None);
}

/// A slice keeps its rows' codes; a slice that drops the section loses them (the check that
/// would catch a merge dropping it).
#[test]
fn a_slice_keeps_its_rows_codes() {
    let m = coded();
    let whole = m.encode();
    let pack = Pack::open(&whole).unwrap();
    let keep: Vec<u32> = (0..m.ids.len() as u32).filter(|r| r % 3 != 1).collect();
    let bodies = extract::rows(&pack, &keep, 0, &Options::default()).unwrap();
    let write = |skip: Option<Kind>| {
        let list: Vec<(Kind, Encoding, u64, &[u8])> = bodies
            .iter()
            .filter(|b| Some(b.0) != skip)
            .map(|b| (b.0, b.1, b.2, b.3.as_slice()))
            .collect();
        let w = Writer::new("np-slice", "places", "2026.11.0").unwrap();
        w.pack(&list, None).unwrap()
    };
    let kept = |bytes: &[u8]| {
        let s = Model::from_pack(bytes);
        let mut v: Vec<(u64, IsoCode)> = s
            .iso
            .iter()
            .map(|&(r, c)| (s.ids[r as usize].get(), c))
            .collect();
        v.sort_unstable();
        v
    };
    let mut want: Vec<(u64, IsoCode)> = m
        .iso
        .iter()
        .filter(|(r, _)| keep.contains(r))
        .map(|&(r, c)| (m.ids[r as usize].get(), c))
        .collect();
    want.sort_unstable();
    assert!(
        want.len() > 1 && want.len() < m.iso.len(),
        "a slice of some coded rows"
    );
    assert_eq!(kept(&write(None)), want);
    assert_eq!(kept(&write(Some(Kind::ISO_CODES))), []);
}

/// The section's size for a country pack: the country and seven provinces spread over 97,000
/// rows, as Nepal's (estimated at about 0.6 kB in the note).
#[test]
fn a_country_packs_its_codes_in_well_under_a_kilobyte() {
    let codes: Vec<(u32, IsoCode)> = [
        (1_234, "NP-P7"),
        (15_000, "NP-P6"),
        (29_001, "NP-P5"),
        (41_000, "NP-P4"),
        (50_500, "NP"),
        (52_000, "NP-P3"),
        (70_000, "NP-P2"),
        (96_999, "NP-P1"),
    ]
    .iter()
    .map(|&(r, c)| (r, code(c)))
    .collect();
    let body = teigeodata_format::iso_codes::encode(&codes)
        .unwrap()
        .unwrap();
    assert!(body.len() < 700, "{} B", body.len());
}
