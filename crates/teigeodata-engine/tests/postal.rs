//! Postal codes (D-200): a code to its places in any input form, autocomplete, a place's
//! codes, a point's codes through the deepest admin unit of an admin-aligned scheme, the first
//! layer holding a code answering it, replacements, and codes that were not installed.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty,
    reason = "tests: failures panic with their message; row counts are small"
)]

mod common;

use teigeodata_core::{E7, Point};
use teigeodata_engine::{
    CodeKind, Component, Components, Engine, Error, Layer, LayerOptions, PostalCode, Scratch,
    Verify,
};
use teigeodata_format::Kind;
use teigeodata_format::postal::{self, Code, Rule};

/// Today's `np` scheme (admin-aligned: 5 digits a district here, 7 its wards) and a retired
/// `old` scheme that is not admin-aligned.
fn rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "np".into(),
            admin_aligned: true,
            ignore: " -".into(),
            templates: vec!["99999".into(), "9999999".into()],
            ..Rule::default()
        },
        Rule {
            name: "old".into(),
            ignore: " -".into(),
            valid_to: Some(20_000),
            templates: vec!["9999".into()],
            ..Rule::default()
        },
    ]
}

fn code(scheme: &str, text: &str, place: u32, kind: CodeKind) -> Code {
    Code {
        scheme: scheme.into(),
        code: text.into(),
        place,
        kind,
        valid_from: None,
        valid_to: None,
        redirect: None,
    }
}

/// District `d`'s area code: 1060`d`.
fn area(d: usize) -> String {
    format!("1060{d}")
}

/// The synthetic country with codes: every district an `np` area code and two wards; district
/// 0 also an `old` code replaced by its `np` one; the first settlement a post office code.
fn model(with_codes: &[usize]) -> (common::Model, Vec<u32>) {
    let mut m = common::synthetic(200, 7);
    let districts: Vec<u32> = m.admin.as_ref().unwrap().rows[1..].to_vec();
    let mut codes = Vec::new();
    for &d in with_codes {
        let row = districts[d];
        codes.push(code("np", &area(d), row, CodeKind::Area));
        for w in 1..=2 {
            codes.push(code("np", &format!("{}0{w}", area(d)), row, CodeKind::Part));
        }
    }
    if with_codes.contains(&0) {
        codes.push(Code {
            redirect: Some(("np".into(), area(0))),
            ..code("old", "4460", districts[0], CodeKind::Area)
        });
    }
    let settlement = (0..m.ids.len() as u32)
        .find(|&r| m.levels[r as usize] == 0 && !districts.contains(&r) && r != 0)
        .unwrap();
    codes.push(code("np", "4460099", settlement, CodeKind::PostOffice));
    let places = u32::try_from(m.ids.len()).unwrap();
    let body = postal::encode(&rules(), &codes, places, true).unwrap();
    m.raw.push((Kind::POSTAL, codes.len() as u64, body));
    (m, districts)
}

fn layer(bytes: Vec<u8>, installed: Option<Components>) -> Layer {
    let options = LayerOptions {
        installed,
        verify: Verify::Lazy,
        ..LayerOptions::default()
    };
    Layer::from_bytes(bytes, options).unwrap()
}

fn texts(codes: &[PostalCode]) -> Vec<(String, String)> {
    codes
        .iter()
        .map(|c| (c.scheme().to_owned(), c.code().to_owned()))
        .collect()
}

#[test]
fn a_code_names_its_places_in_any_form() {
    let (m, districts) = model(&[0, 1, 2, 3]);
    let mut e = Engine::new();
    e.add(layer(m.encode(), None));
    let mut out = Vec::new();
    for input in ["10601", " 10601 ", "106-01", "１０６０１", "१०६०१"] {
        e.postal_lookup(input, Some("np"), &mut out).unwrap();
        assert_eq!(out.len(), 1, "{input:?}");
        assert_eq!((out[0].scheme(), out[0].code()), ("np", "10601"));
        assert_eq!(out[0].place.row, districts[1]);
        assert_eq!(out[0].kind, CodeKind::Area);
    }
    // A ward is a part of its district, not a place of its own.
    e.postal_lookup("1060102", None, &mut out).unwrap();
    assert_eq!(
        (out[0].place.row, out[0].kind),
        (districts[1], CodeKind::Part)
    );
    // No template accepts it, or no such code: nothing, not an error.
    for input in ["1060", "106011", "10609", "abcde", ""] {
        e.postal_lookup(input, Some("np"), &mut out).unwrap();
        assert!(out.is_empty(), "{input:?}");
    }
    // The retired scheme's code, its validity and its replacement.
    e.postal_lookup("4460", None, &mut out).unwrap();
    assert_eq!(texts(&out), [("old".into(), "4460".into())]);
    let old = out[0];
    assert_eq!(old.valid_to, Some(20_000));
    assert!(old.valid_on(19_000) && !old.valid_on(20_001));
    assert!(old.is_replaced());
    let mut next = Vec::new();
    e.postal_replacement(&old, &mut next).unwrap();
    assert_eq!(texts(&next), [("np".into(), "10600".into())]);
    assert_eq!(next[0].place.id, old.place.id);
}

#[test]
fn autocomplete_lists_codes_in_order_up_to_the_limit() {
    let (m, _) = model(&[0, 1, 2, 3]);
    let mut e = Engine::new();
    e.add(layer(m.encode(), None));
    let mut out = Vec::new();
    e.postal_prefix("10601", Some("np"), 10, &mut out).unwrap();
    assert_eq!(
        texts(&out).into_iter().map(|t| t.1).collect::<Vec<_>>(),
        ["10601", "1060101", "1060102"]
    );
    e.postal_prefix("106", None, 4, &mut out).unwrap();
    assert_eq!(out.len(), 4);
    assert_eq!(out[0].code(), "10600");
    e.postal_prefix("  ", None, 4, &mut out).unwrap();
    assert!(out.is_empty());
}

#[test]
fn a_place_has_its_codes_and_a_point_its_units() {
    let (m, districts) = model(&[0, 1, 2, 3]);
    let points = m.points.clone();
    let mut e = Engine::new();
    e.add(layer(m.encode(), None));
    let mut out = Vec::new();
    let d0 = e
        .postal_lookup("10600", Some("np"), &mut out)
        .map(|()| out[0].place)
        .unwrap();
    e.postal_codes_of(&d0, &mut out).unwrap();
    assert_eq!(
        texts(&out),
        [
            ("np".into(), "10600".into()),
            ("np".into(), "1060001".into()),
            ("np".into(), "1060002".into()),
            ("old".into(), "4460".into()),
        ]
    );
    // At the district's centre: its codes, not the retired scheme's (not admin-aligned).
    let mut s = Scratch::default();
    let at = points[districts[0] as usize].to_e7();
    let unit = e.postal_at(at, &mut s, &mut out).unwrap().unwrap();
    assert_eq!(unit.row, districts[0]);
    assert_eq!(
        texts(&out).into_iter().map(|t| t.1).collect::<Vec<_>>(),
        ["10600", "1060001", "1060002"]
    );
    // A district without codes: none of its own, and the country has none either.
    let at = points[districts[5] as usize].to_e7();
    assert_eq!(e.postal_at(at, &mut s, &mut out).unwrap(), None);
    assert!(out.is_empty());
    // Outside every unit.
    let far = Point::<E7>::from_units(0, 0).unwrap();
    assert_eq!(e.postal_at(far, &mut s, &mut out).unwrap(), None);
}

#[test]
fn the_first_layer_holding_a_code_answers() {
    // The upper layer has codes for districts 0 and 1 only; the lower one for 0 to 3.
    let (upper, _) = model(&[0, 1]);
    let (mut lower, districts) = model(&[0, 1, 2, 3]);
    lower.address = "np-all".into();
    let mut upper = upper;
    upper.address = "np".into();
    upper.release = "2026.12.0".into();
    let mut e = Engine::new();
    e.add(layer(upper.encode(), None));
    e.add(layer(lower.encode(), None));
    let ids: Vec<_> = e.layers().map(Layer::id).collect();
    let mut out = Vec::new();
    e.postal_lookup("10601", None, &mut out).unwrap();
    assert_eq!(out.len(), 1, "a code in two layers answers once");
    assert_eq!(out[0].layer, ids[0]);
    e.postal_lookup("10603", None, &mut out).unwrap();
    assert_eq!((out.len(), out[0].layer), (1, ids[1]));
    // The district's codes fall through to the layer that has them; its place is the
    // winning layer's.
    let d3 = out[0].place;
    assert_eq!(d3.layer, ids[0]);
    assert_eq!(d3.row, districts[3]);
    e.postal_codes_of(&d3, &mut out).unwrap();
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|c| c.layer == ids[1]));
}

#[test]
fn codes_not_installed_are_named_not_skipped() {
    let (m, _) = model(&[0, 1]);
    let bytes = m.encode();
    let mut e = Engine::new();
    e.add(layer(
        bytes.clone(),
        Some(Components::of(&[Component::Point])),
    ));
    let mut out = Vec::new();
    match e.postal_lookup("10601", None, &mut out) {
        Err(Error::MissingComponent { component, .. }) => {
            assert_eq!(component, Component::Postal);
        }
        other => panic!("codes not installed answered {other:?}"),
    }
    // Not installed in one layer, installed in another that has the code: it answers.
    let (mut other, _) = model(&[0, 1]);
    other.address = "np-all".into();
    let mut e = Engine::new();
    e.add(layer(bytes, Some(Components::of(&[Component::Point]))));
    e.add(layer(
        other.encode(),
        Some(Components::of(&[Component::Point, Component::Postal])),
    ));
    e.postal_lookup("10601", None, &mut out).unwrap();
    assert_eq!(out.len(), 1);
    // A place's codes: the layer holding its ID without codes installed is named.
    let mut e2 = Engine::new();
    e2.add(layer(m.encode(), Some(Components::of(&[Component::Point]))));
    let d = e
        .postal_lookup("10601", None, &mut out)
        .map(|()| out[0].place)
        .unwrap();
    assert!(matches!(
        e2.postal_codes_of(&d, &mut out),
        Err(Error::MissingComponent { .. })
    ));
}
