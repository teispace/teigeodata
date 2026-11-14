//! Substring search (D-199, TG-06): a name is found from 3 or more code points inside a word,
//! Latin and Devanagari, ranked below exact and prefix matches; only layers installed with the
//! `substring` component add such matches; a row a higher layer claims stays hidden.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    reason = "tests: failures panic with their message"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{Model, OwnedName};
use teigeodata_core::{E7, Namespace, PlaceId, PlaceKind, Point};
use teigeodata_engine::{Component, Components, Engine, Layer, LayerOptions, Scratch, Verify};
use teigeodata_format::search::{EXACT, LOOSE};
use teigeodata_names::search_keys;

/// A place: its ID serial, names and importance.
type Spec<'a> = (u64, &'a [&'a str], u8);

/// A pack of `places` keyed as the forge keys them (SEARCH's exact and loose maps, and
/// SUBSTRING when `substring`), all at one point.
fn pack(address: &str, places: &[Spec<'_>], substring: bool) -> Vec<u8> {
    let ns = Namespace::from_country(*b"np").unwrap();
    let n = places.len();
    let mut keys: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    for (row, (_, names, _)) in places.iter().enumerate() {
        let row = row as u32;
        for name in *names {
            search_keys(name, &[], None, |m, k| {
                let tag = match m {
                    teigeodata_names::SearchMap::Exact => EXACT,
                    teigeodata_names::SearchMap::Loose => LOOSE,
                    teigeodata_names::SearchMap::Phonetic => return,
                };
                let mut key = vec![tag];
                key.extend_from_slice(k.as_bytes());
                keys.entry(key).or_default().insert(row);
            });
        }
    }
    // A loose key keeps only the rows its exact twin lacks (D-086).
    let exact: BTreeMap<Vec<u8>, BTreeSet<u32>> = keys
        .iter()
        .filter(|(k, _)| k[0] == EXACT)
        .map(|(k, v)| (k[1..].to_vec(), v.clone()))
        .collect();
    keys.retain(|k, rows| {
        if k[0] == LOOSE
            && let Some(twin) = exact.get(&k[1..])
        {
            rows.retain(|r| !twin.contains(r));
        }
        !rows.is_empty()
    });
    let importance: Vec<u8> = places.iter().map(|p| p.2).collect();
    // One point for all: the rows need no Hilbert order then.
    let point = Point::<E7>::from_units(277_000_000, 853_000_000)
        .unwrap()
        .to_e6();
    let mut m = Model {
        address: address.into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        bbox: None,
        ids: places
            .iter()
            .map(|p| PlaceId::new(ns, p.0).unwrap())
            .collect(),
        points: vec![point; n],
        kinds: vec![PlaceKind::Settlement; n],
        levels: vec![0; n],
        classes: vec![0; n],
        flags: vec![0; n],
        elevations: vec![None; n],
        names: places
            .iter()
            .map(|p| {
                p.1.iter()
                    .map(|t| OwnedName {
                        text: (*t).to_string(),
                        lang: None,
                        script: None,
                        kind: None,
                        valid: (None, None),
                        rule: 0,
                    })
                    .collect()
            })
            .collect(),
        keys,
        importance,
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
    if substring {
        m.with_substring();
    }
    m.encode()
}

fn layer(bytes: Vec<u8>, installed: Option<Components>, priority: i32) -> Layer {
    let options = LayerOptions {
        installed,
        priority,
        verify: Verify::Lazy,
        ..LayerOptions::default()
    };
    Layer::from_bytes(bytes, options).unwrap()
}

/// The serials and scores `query` returns, best first.
fn found(e: &Engine, query: &str) -> Vec<(u64, i32)> {
    let mut s = Scratch::default();
    let mut out = Vec::new();
    e.search(query, 10, &mut s, &mut out).unwrap();
    out.iter().map(|h| (h.place.id.serial(), h.score)).collect()
}

fn serials(e: &Engine, query: &str) -> Vec<u64> {
    found(e, query).into_iter().map(|h| h.0).collect()
}

const KATHMANDU: u64 = 1;
const POKHARA: u64 = 2;
const BHIMPOKHARA: u64 = 3;
const BIRATNAGAR: u64 = 4;
const MANDU: u64 = 5;

const KATNAGAR: u64 = 8;

const NEPAL: [Spec<'static>; 7] = [
    (KATHMANDU, &["Kathmandu", "काठमाडौं", "Kantipur"], 200),
    (POKHARA, &["Pokhara", "पोखरा"], 200),
    (BHIMPOKHARA, &["Bhimpokhara"], 60),
    (BIRATNAGAR, &["Biratnagar", "बिराटनगर"], 140),
    (MANDU, &["Mandu"], 10),
    (6, &["Lalitpur"], 120),
    (KATNAGAR, &["Katnagar"], 50),
];

fn nepal(substring: bool) -> Engine {
    let mut e = Engine::new();
    e.add(layer(pack("np", &NEPAL, substring), None, 0));
    e
}

#[test]
fn a_name_is_found_from_the_middle_of_a_word() {
    let e = nepal(true);
    // Latin and Devanagari, mid-word: 20 plus the importance.
    assert_eq!(found(&e, "athmand"), [(KATHMANDU, 220)]);
    assert_eq!(found(&e, "माडौं"), [(KATHMANDU, 220)]);
    // A substring match is not strong: the pass over every first letter still runs and finds
    // Katnagar one edit away (25 + 50).
    assert_eq!(found(&e, "ratnagar"), [(BIRATNAGAR, 160), (KATNAGAR, 75)]);
    assert_eq!(serials(&e, "राटनगर"), [BIRATNAGAR]);
    // 3 code points are enough, and काठ is 2 graphemes: found by the exact prefix range.
    assert_eq!(serials(&e, "काठ"), [KATHMANDU]);
    // "pur" ends Kantipur and Lalitpur.
    let pur = serials(&e, "pur");
    assert!(pur.contains(&KATHMANDU) && pur.contains(&6), "{pur:?}");
    // Fewer than 3 code points, or a space: no substring tier.
    assert_eq!(serials(&e, "du"), Vec::<u64>::new());
    assert!(!serials(&e, "ath mandu").contains(&KATHMANDU));
    // Without the section, nothing from the middle of a word.
    let plain = nepal(false);
    assert_eq!(serials(&plain, "athmand"), Vec::<u64>::new());
    assert_eq!(serials(&plain, "राटनगर"), Vec::<u64>::new());
}

#[test]
fn two_code_points_are_a_prefix() {
    // A half-typed syllable lists its names while typing (TG-18, D-236): 2 code points, Latin
    // or Devanagari (पो is one akshara), are a prefix (30); one is not.
    let e = nepal(false);
    assert_eq!(found(&e, "po"), [(POKHARA, 230)]);
    assert_eq!(found(&e, "पो"), [(POKHARA, 230)]);
    assert_eq!(serials(&e, "p"), Vec::<u64>::new());
}

#[test]
fn an_exact_match_stays_first() {
    let e = nepal(true);
    // Pokhara exact (60 + 200) above Bhimpokhara from the middle (20 + 60).
    assert_eq!(found(&e, "Pokhara"), [(POKHARA, 260), (BHIMPOKHARA, 80)]);
    // A row found as a prefix (30) and from the middle keeps the higher bonus; 4 code points
    // from the middle take 15, 5 or more 20 (D-269).
    assert_eq!(found(&e, "pokh"), [(POKHARA, 230), (BHIMPOKHARA, 75)]);
    assert_eq!(found(&e, "okh"), [(POKHARA, 210), (BHIMPOKHARA, 70)]);
    // khara: both from the middle; importance decides.
    assert_eq!(found(&e, "khara"), [(POKHARA, 220), (BHIMPOKHARA, 80)]);
    assert_eq!(serials(&e, "खरा"), [POKHARA]);
    // One additive score (D-052): an important place found from the middle of a word still
    // outranks an unimportant exact match (Kathmandu 20 + 200 above Mandu 60 + 10).
    assert_eq!(found(&e, "mandu"), [(KATHMANDU, 220), (MANDU, 70)]);
}

#[test]
fn only_layers_installed_with_the_component_add_substring_matches() {
    // The pack has SUBSTRING; installed without it, search still answers, from SEARCH alone.
    let set = Components::of(&[Component::Point, Component::Names, Component::Search]);
    let mut e = Engine::new();
    e.add(layer(pack("np", &NEPAL, true), Some(set), 0));
    assert_eq!(serials(&e, "athmand"), Vec::<u64>::new());
    assert_eq!(serials(&e, "Kathmandu"), [KATHMANDU]);
    let installed = e.layers().next().unwrap().components();
    assert!(installed.contains(Component::Search) && !installed.contains(Component::Substring));
    // With it, it does.
    let mut e = Engine::new();
    let with = set.union(Components::of(&[Component::Substring]));
    e.add(layer(pack("np", &NEPAL, true), Some(with), 0));
    assert_eq!(serials(&e, "athmand"), [KATHMANDU]);
    // The component without search is refused, naming what it needs.
    let lone = Components::of(&[Component::Point, Component::Names, Component::Substring]);
    let refused = Layer::from_bytes(
        pack("np", &NEPAL, true),
        LayerOptions {
            installed: Some(lone),
            ..LayerOptions::default()
        },
    );
    assert!(matches!(
        refused.err(),
        Some(teigeodata_engine::Error::MissingComponent {
            component: Component::Search,
            ..
        })
    ));
}

#[test]
fn a_row_a_higher_layer_claims_stays_hidden() {
    // The delta (higher priority) renames Kathmandu; the base still has the old name with
    // its substring keys. "athmand" must not return the base's row for that ID.
    let mut e = Engine::new();
    e.add(layer(pack("np", &NEPAL, true), None, 0));
    e.add(layer(
        pack("np-delta", &[(KATHMANDU, &["Yen"], 200)], true),
        None,
        10,
    ));
    assert_eq!(serials(&e, "athmand"), Vec::<u64>::new());
    // Other rows of the base are still found, and the delta's own name from its middle.
    assert_eq!(serials(&e, "ratnagar"), [BIRATNAGAR, KATNAGAR]);
    let mut e = Engine::new();
    e.add(layer(pack("np", &NEPAL, true), None, 0));
    e.add(layer(
        pack("np-delta", &[(7, &["Thulopokhara"], 255)], true),
        None,
        10,
    ));
    // Across layers the best by importance wins: the delta's row first.
    assert_eq!(serials(&e, "khara"), [7, POKHARA, BHIMPOKHARA]);
    // Claimed rows take no place in the tier's best 16: twenty important base rows the delta
    // renames must not crowd out the one visible, unimportant row.
    let names: Vec<String> = (0..20).map(|i| format!("Pokhara{i:02}")).collect();
    let refs: Vec<[&str; 1]> = names.iter().map(|n| [n.as_str()]).collect();
    let mut base: Vec<Spec<'_>> = (0..20)
        .map(|i| (100 + i as u64, &refs[i][..], 250))
        .collect();
    base.push((99, &["Bhimpokhara"], 1));
    let renamed: Vec<Spec<'_>> = (0..20).map(|i: u64| (100 + i, &["Yen"][..], 10)).collect();
    let mut e = Engine::new();
    e.add(layer(pack("np", &base, true), None, 0));
    e.add(layer(pack("np-delta", &renamed, true), None, 10));
    assert_eq!(serials(&e, "khara"), [99]);
}
