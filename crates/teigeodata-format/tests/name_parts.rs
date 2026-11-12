//! Names and search by language (ADR-0040, step 2): the rule that picks the parts, a pack with
//! NAMES and SEARCH split by language read back through [`NameSet`] and [`SearchSet`], and the
//! faults each refuses.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a panic reports a failure, and indexes are into data built here"
)]

use std::collections::{BTreeMap, BTreeSet};

use teigeodata_format::names::{self, LanguageParts, NameRecord, NameSet, PART_MIN_TEXT, Tag};
use teigeodata_format::search::{self, EXACT, Extra, Maps, Search, SearchSet};
use teigeodata_format::{Encoding, Kind, Pack, Part, Writer};

fn tag(t: &str) -> Tag {
    Tag::new(t).unwrap_or_else(|| panic!("{t} packs"))
}

fn lang(t: &str) -> Part {
    Part::language(t).unwrap_or_else(|| panic!("{t} packs"))
}

#[test]
fn a_language_gets_a_part_from_its_threshold_and_the_rest_share_one() {
    let (ne, mai) = (Some(tag("ne")), Some(tag("mai")));
    // `ne` reaches the threshold only by names that are not canonical; `mai` stays below.
    let places = [
        vec![
            (ne, PART_MIN_TEXT),
            (ne, PART_MIN_TEXT),
            (mai, 10),
            (None, 5),
        ],
        vec![(None, 4), (mai, 10)],
    ];
    let p = LanguageParts::of(places.iter().map(|p| p.iter().copied()));
    assert_eq!(p.parts(), &[Part::BASE, Part::REST, lang("ne")]);
    assert!(!p.is_whole());
    // A canonical name and a name with no language stay in the base part.
    assert_eq!(p.part_of(ne, true), Part::BASE);
    assert_eq!(p.part_of(None, false), Part::BASE);
    assert_eq!(p.part_of(ne, false), lang("ne"));
    assert_eq!(p.part_of(mai, false), Part::REST);
    // One byte below: no part of its own.
    let below = [vec![(None, 4), (ne, PART_MIN_TEXT - 1)]];
    let p = LanguageParts::of(below.iter().map(|p| p.iter().copied()));
    assert_eq!(p.parts(), &[Part::BASE, Part::REST]);
    // No names but canonical ones: one part, the section is not split.
    let whole = [vec![(ne, 100_000)]];
    assert!(LanguageParts::of(whole.iter().map(|p| p.iter().copied())).is_whole());
    assert!(LanguageParts::default().is_whole());
}

fn refs<'p, 'a>(p: &'p [Vec<NameRecord<'a>>]) -> Vec<&'p [NameRecord<'a>]> {
    p.iter().map(Vec::as_slice).collect()
}

fn rec<'a>(text: &'a str, l: Option<&str>) -> NameRecord<'a> {
    NameRecord {
        text,
        lang: l.map(tag),
        ..NameRecord::default()
    }
}

fn map(keys: &[(&str, u32)]) -> BTreeMap<String, BTreeSet<u32>> {
    let mut m: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for &(k, r) in keys {
        m.entry(k.to_owned()).or_default().insert(r);
    }
    m
}

/// Two places, names in a base part and an `ne` part, and SEARCH split the same way.
fn split_pack() -> Vec<u8> {
    let base = [
        vec![rec("Kathmandu", None)],
        vec![rec("Pokhara", None), rec("Pokhara", Some("en"))],
    ];
    let ne = [vec![rec("काठमाडौं", Some("ne"))], vec![]];
    let names_base = names::encode(&refs(&base)).unwrap_or_else(|e| panic!("{e}"));
    let names_ne = names::encode_part(&refs(&ne)).unwrap_or_else(|e| panic!("{e}"));
    let importance = [40_u8, 30];
    let base_maps = Maps {
        exact: map(&[("kathmandu", 0), ("pokhara", 1)]),
        ..Maps::default()
    };
    let ne_maps = Maps {
        exact: map(&[("काठमाडौं", 0)]),
        ..Maps::default()
    };
    let search_base = search::encode_maps_with(&base_maps, &importance, &Extra::default())
        .unwrap_or_else(|e| panic!("{e}"));
    let part = Extra {
        part: true,
        ..Extra::default()
    };
    let search_ne =
        search::encode_maps_with(&ne_maps, &importance, &part).unwrap_or_else(|e| panic!("{e}"));
    Writer::new("np", "places", "2026.11.0")
        .unwrap_or_else(|e| panic!("{e}"))
        .pack_parts(
            &[
                (Kind::NAMES, Part::BASE, Encoding::TABLE, 2, &names_base),
                (Kind::NAMES, lang("ne"), Encoding::TABLE, 2, &names_ne),
                (Kind::SEARCH, Part::BASE, Encoding::TABLE, 2, &search_base),
                (Kind::SEARCH, lang("ne"), Encoding::TABLE, 2, &search_ne),
            ],
            None,
        )
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn names_and_search_read_back_across_their_parts() {
    let bytes = split_pack();
    let pack = Pack::open(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let names = NameSet::open(pack.parts(Kind::NAMES)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(names.parts().len(), 2);
    let mut buf = [0_u8; names::TEXT_BUFFER];
    let texts = |row: usize, buf: &mut [u8]| -> Vec<String> {
        names
            .names(row)
            .map(|n| n.text(buf).map(str::to_owned))
            .collect::<Result<_, _>>()
            .unwrap_or_else(|e| panic!("{e}"))
    };
    assert_eq!(texts(0, &mut buf), ["Kathmandu", "काठमाडौं"]);
    assert_eq!(texts(1, &mut buf), ["Pokhara", "Pokhara"]);
    let canonical = names
        .canonical(0)
        .map(|n| n.text(&mut buf).map(str::to_owned));
    assert_eq!(canonical, Some(Ok("Kathmandu".to_owned())));

    let set = SearchSet::open(pack.parts(Kind::SEARCH)).unwrap_or_else(|e| panic!("{e}"));
    let [base, ne] = set.parts() else {
        panic!("two search parts")
    };
    // The part has no importance of its own: it reads the base's.
    assert_eq!((ne.importance(0), ne.importance(1)), (40, 30));
    assert!(base.get(EXACT, "kathmandu").is_some());
    assert!(base.get(EXACT, "काठमाडौं").is_none());
    assert!(ne.get(EXACT, "काठमाडौं").is_some());
    for s in set.parts() {
        s.check().unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn a_part_without_its_base_or_out_of_place_is_refused() {
    let bytes = split_pack();
    let pack = Pack::open(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let part = |k: Kind| {
        pack.part(k, lang("ne"))
            .unwrap_or_else(|| panic!("ne part"))
    };
    // A NAMES part alone, or first: no base part to hold the canonical names.
    assert!(NameSet::open([part(Kind::NAMES)]).is_err());
    // A SEARCH part opened as a base part has no importance.
    assert!(Search::open(&part(Kind::SEARCH)).is_err());
    assert!(SearchSet::open([part(Kind::SEARCH)]).is_err());
    // A part carries no rules or unit types: those live in the base.
    let units = [1_u8, 0];
    let bad = Extra {
        part: true,
        units: &units,
        ..Extra::default()
    };
    assert!(search::encode_maps_with(&Maps::default(), &[1, 1], &bad).is_err());
    // A base NAMES part needs every place's canonical name.
    let empty: [&[NameRecord<'_>]; 1] = [&[]];
    assert!(names::encode(&empty).is_err());
    assert!(names::encode_part(&empty).is_ok());
}
