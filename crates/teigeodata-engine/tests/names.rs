//! A place's names by language (D-227): `name_in` takes a language or a language and script,
//! prefers a name a person wrote over a machine one, and `names` lists every name with what it
//! is.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message"
)]

mod common;

use std::collections::BTreeMap;

use common::{Model, OwnedName};
use teigeodata_core::{E7, Namespace, PlaceId, PlaceKind, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Origin, Tag, Verify};

fn name(text: &str, lang: Option<&str>, script: Option<&str>, origin: Origin) -> OwnedName {
    OwnedName {
        text: text.into(),
        lang: lang.and_then(Tag::new),
        script: script.and_then(Tag::new),
        kind: None,
        valid: (None, None),
        rule: origin.code(),
    }
}

/// One place: its canonical Latin name, a machine Nepali name stored before a human one, and
/// a romanisation tagged `ne`.
fn engine() -> Engine {
    let ns = Namespace::from_country(*b"np").unwrap();
    let point = Point::<E7>::from_units(277_000_000, 853_000_000)
        .unwrap()
        .to_e6();
    let m = Model {
        address: "np".into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        bbox: None,
        ids: vec![PlaceId::new(ns, 1).unwrap()],
        points: vec![point],
        kinds: vec![PlaceKind::Settlement],
        levels: vec![0],
        classes: vec![0],
        flags: vec![0],
        elevations: vec![None],
        names: vec![vec![
            name("Biratnagar", None, Some("Latn"), Origin::GeoNames),
            name(
                "बिराटनगर",
                Some("ne"),
                Some("Deva"),
                Origin::Transliteration,
            ),
            name("विराटनगर", Some("ne"), Some("Deva"), Origin::Wikidata),
            name("Birāṭnagar", Some("ne"), Some("Latn"), Origin::Romanisation),
        ]],
        keys: BTreeMap::new(),
        importance: vec![100],
        rules: None,
        units: Vec::new(),
        node: 8,
        redirects: Vec::new(),
        zones: None,
        admin: None,
        arcs: None,
        parents: Vec::new(),
        iso: Vec::new(),
        raw: Vec::new(),
    };
    let options = LayerOptions {
        verify: Verify::Lazy,
        ..LayerOptions::default()
    };
    let mut e = Engine::new();
    e.add(Layer::from_bytes(m.encode(), options).unwrap());
    e
}

#[test]
fn a_name_in_a_language_prefers_a_person_over_a_machine() {
    let e = engine();
    let place = e
        .get(PlaceId::new(Namespace::from_country(*b"np").unwrap(), 1).unwrap())
        .unwrap()
        .unwrap();
    let mut buf = [0; 256];
    let (text, info) = e.name_in(&place, "ne-Deva", &mut buf).unwrap().unwrap();
    assert_eq!(text, "विराटनगर");
    assert_eq!(info.origin, Origin::Wikidata);
    assert!(!info.origin.is_machine());
    // `ne` alone also takes the Latin romanisation, still after the human Devanagari name.
    let (text, _) = e.name_in(&place, "ne", &mut buf).unwrap().unwrap();
    assert_eq!(text, "विराटनगर");
    let (text, info) = e.name_in(&place, "ne-Latn", &mut buf).unwrap().unwrap();
    assert_eq!((text, info.origin.is_machine()), ("Birāṭnagar", true));
    assert!(e.name_in(&place, "hi", &mut buf).unwrap().is_none());
    assert!(
        e.name_in(&place, "ne-Dev", &mut buf).unwrap().is_none(),
        "not a script"
    );

    let mut all = Vec::new();
    e.names(&place, &mut buf, |t, i| all.push((t.to_owned(), i.origin)))
        .unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0], ("Biratnagar".to_owned(), Origin::GeoNames));
    assert_eq!(all[1].1, Origin::Transliteration);
}
