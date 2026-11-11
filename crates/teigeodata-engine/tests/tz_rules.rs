//! Zone rules (D-196, TG-03): `tz_offset_at`, `tz_local_candidates`, `tz_version` and
//! `tz_offset_at_point` answer from the first layer by precedence whose rules hold the zone,
//! read gaps and overlaps by definition, follow an area to its zone through its row, and
//! refuse rules that were not installed.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests: failures panic with their message"
)]

mod common;

use teigeodata_core::{E7, Point};
use teigeodata_engine::{
    Civil, Component, Components, Engine, Error, Layer, LayerOptions, Local, Verify,
};
use teigeodata_format::tz_rules::{self, TypeRow, ZoneInput, ZoneRules};
use teigeodata_format::{Encoding, Kind, Tag, Writer};

/// Kathmandu as tzdb has it: LMT, +0530 from 1920, +0545 from 1986.
fn kathmandu() -> ZoneRules {
    let t = |offset, abbr: &str| TypeRow {
        offset,
        is_dst: false,
        abbr: abbr.into(),
    };
    ZoneRules {
        version: 2,
        transitions: vec![(-1_577_923_200, 1), (504_901_800, 2)],
        types: vec![t(20_476, "LMT"), t(19_800, "+0530"), t(20_700, "+0545")],
        footer: tz_rules::parse_footer("<+0545>-5:45").unwrap(),
        first_abbr: 0,
    }
}

/// A zone of fixed offset `offset` named `abbr`, no transitions.
fn fixed(offset: i32, abbr: &str) -> ZoneRules {
    let footer = format!("<{abbr}>-{}:{:02}", offset / 3600, offset % 3600 / 60);
    ZoneRules {
        version: 2,
        transitions: Vec::new(),
        types: vec![TypeRow {
            offset,
            is_dst: false,
            abbr: abbr.into(),
        }],
        footer: tz_rules::parse_footer(&footer).unwrap(),
        first_abbr: 0,
    }
}

/// New York from its footer alone (US rules since 2007).
fn new_york() -> ZoneRules {
    ZoneRules {
        version: 2,
        transitions: Vec::new(),
        types: vec![TypeRow {
            offset: -18_000,
            is_dst: false,
            abbr: "EST".into(),
        }],
        footer: tz_rules::parse_footer("EST5EDT,M3.2.0,M11.1.0").unwrap(),
        first_abbr: 0,
    }
}

/// A pack of zone rules only, its manifest naming tzdb `release`.
fn rules_pack(address: &str, zones: &[(&str, &ZoneRules)], release: Option<&str>) -> Vec<u8> {
    let inputs: Vec<ZoneInput<'_>> = zones
        .iter()
        .map(|&(name, rules)| ZoneInput {
            name,
            rules,
            place: None,
        })
        .collect();
    let body = tz_rules::encode(&inputs, false).unwrap();
    let mut w = Writer::new(address, "places", "2026.11.0").unwrap();
    if let Some(r) = release {
        w.manifest(Tag::TZDB, r.as_bytes()).unwrap();
    }
    w.pack(
        &[(Kind::TZ_RULES, Encoding::TABLE, zones.len() as u64, &body)],
        None,
    )
    .unwrap()
}

fn layer(bytes: Vec<u8>, installed: Option<Components>) -> Layer {
    let options = LayerOptions {
        installed,
        verify: Verify::Lazy,
        ..LayerOptions::default()
    };
    Layer::from_bytes(bytes, options).unwrap()
}

fn civil(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> Civil {
    Civil {
        year,
        month,
        day,
        hour,
        minute,
        second: 0,
    }
}

#[test]
fn offsets_follow_history_then_the_footer() {
    let (ktm, ny) = (kathmandu(), new_york());
    let mut e = Engine::new();
    e.add(layer(
        rules_pack(
            "np",
            &[("Asia/Kathmandu", &ktm), ("America/New_York", &ny)],
            Some("2026f"),
        ),
        None,
    ));
    let at = |zone, t| e.tz_offset_at(zone, t).unwrap().unwrap();
    // 1900 (LMT), 1980 (+0530), 2024 (+0545, from the footer past the last transition).
    let o = at("Asia/Kathmandu", -2_208_988_800);
    assert_eq!((o.seconds, o.abbr(), o.is_dst), (20_476, "LMT", false));
    assert_eq!(at("Asia/Kathmandu", 315_532_800).seconds, 19_800);
    assert_eq!(at("Asia/Kathmandu", 504_901_799).abbr(), "+0530");
    assert_eq!(at("Asia/Kathmandu", 504_901_800).abbr(), "+0545");
    assert_eq!(at("Asia/Kathmandu", 1_720_000_000).seconds, 20_700);
    // New York: EDT in July, EST in January.
    let july = at("America/New_York", 1_719_835_200);
    assert_eq!(
        (july.seconds, july.abbr(), july.is_dst),
        (-14_400, "EDT", true)
    );
    assert_eq!(at("America/New_York", 1_704_067_200).abbr(), "EST");
    assert_eq!(e.tz_offset_at("Europe/Nowhere", 0).unwrap(), None);
    assert_eq!(e.tz_version(), Some("2026f"));
}

#[test]
fn civil_times_answer_unique_gap_or_overlap() {
    let (ktm, ny) = (kathmandu(), new_york());
    let mut e = Engine::new();
    e.add(layer(
        rules_pack(
            "np",
            &[("Asia/Kathmandu", &ktm), ("America/New_York", &ny)],
            Some("2026f"),
        ),
        None,
    ));
    let local = |zone, c| e.tz_local_candidates(zone, c).unwrap().unwrap();
    let ny = "America/New_York";
    assert_eq!(local(ny, civil(2024, 7, 1, 12, 0)), Local::Unique(-14_400));
    assert_eq!(
        local(ny, civil(2024, 3, 10, 2, 30)),
        Local::Gap {
            before: -18_000,
            after: -14_400
        }
    );
    assert_eq!(
        local(ny, civil(2024, 11, 3, 1, 30)),
        Local::Overlap {
            earlier: -14_400,
            later: -18_000
        }
    );
    // Kathmandu's clocks went from 00:00 to 00:15 on 1986-01-01.
    assert_eq!(
        local("Asia/Kathmandu", civil(1986, 1, 1, 0, 5)),
        Local::Gap {
            before: 19_800,
            after: 20_700
        }
    );
    assert_eq!(
        local("Asia/Kathmandu", civil(1986, 1, 1, 0, 15)),
        Local::Unique(20_700)
    );
    assert_eq!(
        e.tz_local_candidates("Europe/Nowhere", civil(2024, 1, 1, 0, 0))
            .unwrap(),
        None
    );
    // Not a date: refused, not guessed.
    assert!(matches!(
        e.tz_local_candidates(ny, civil(2024, 2, 30, 0, 0)),
        Err(Error::Format(_))
    ));
}

#[test]
fn the_first_layer_holding_the_zone_answers() {
    // The upper layer (a newer release) holds only Kathmandu; the lower one both zones.
    let (upper_ktm, lower_ktm, kol) = (
        fixed(20_700, "+0545"),
        fixed(19_800, "+0530"),
        fixed(19_800, "IST"),
    );
    let mut e = Engine::new();
    e.add(layer(
        rules_pack("np-p3", &[("Asia/Kathmandu", &upper_ktm)], Some("2026f")),
        None,
    ));
    e.add(layer(
        rules_pack(
            "np",
            &[("Asia/Kathmandu", &lower_ktm), ("Asia/Kolkata", &kol)],
            Some("2025a"),
        ),
        None,
    ));
    let ids: Vec<_> = e.layers().map(Layer::id).collect();
    let k = e.tz_offset_at("Asia/Kathmandu", 0).unwrap().unwrap();
    assert_eq!((k.seconds, k.layer), (20_700, ids[0]));
    let i = e.tz_offset_at("Asia/Kolkata", 0).unwrap().unwrap();
    assert_eq!((i.seconds, i.abbr(), i.layer), (19_800, "IST", ids[1]));
    assert_eq!(e.tz_version(), Some("2026f"));
    // A layer without rules has no version to give.
    assert_eq!(Engine::new().tz_version(), None);
}

#[test]
fn rules_not_installed_are_named_not_skipped() {
    let ktm = kathmandu();
    let bytes = rules_pack("np", &[("Asia/Kathmandu", &ktm)], Some("2026f"));
    let mut e = Engine::new();
    e.add(layer(bytes.clone(), Some(Components::NONE)));
    match e.tz_offset_at("Asia/Kathmandu", 0) {
        Err(Error::MissingComponent { component, .. }) => {
            assert_eq!(component, Component::TzRules);
        }
        other => panic!("rules not installed answered {other:?}"),
    }
    assert_eq!(e.tz_version(), None);
    // Installed, it answers.
    let mut e = Engine::new();
    e.add(layer(bytes, Some(Components::of(&[Component::TzRules]))));
    assert_eq!(
        e.tz_offset_at("Asia/Kathmandu", 0)
            .unwrap()
            .unwrap()
            .seconds,
        19_800
    );
}

#[test]
fn a_point_gets_its_areas_zone_and_offset() {
    // The synthetic country's two timezone areas, west and east, linked to two zones.
    let mut model = common::synthetic(300, 11);
    let rows = model.zones.as_ref().unwrap().rows.clone();
    let (west, east) = (kathmandu(), fixed(19_800, "IST"));
    let inputs = [
        ZoneInput {
            name: "Asia/Kathmandu",
            rules: &west,
            place: Some(rows[0]),
        },
        ZoneInput {
            name: "Asia/Kolkata",
            rules: &east,
            place: Some(rows[1]),
        },
    ];
    model
        .raw
        .push((Kind::TZ_RULES, 2, tz_rules::encode(&inputs, false).unwrap()));
    let mut e = Engine::new();
    e.add(layer(model.encode(), None));
    let mid = i32::midpoint(common::SOUTH, common::NORTH);
    let p = |lon| Point::<E7>::from_units(mid, lon).unwrap();
    let when = 1_720_000_000;
    let (area, o) = e
        .tz_offset_at_point(p(common::WEST + 10_000_000), when)
        .unwrap()
        .unwrap();
    assert_eq!((area.row, o.seconds, o.abbr()), (rows[0], 20_700, "+0545"));
    assert_eq!(e.zone_name(&area).unwrap(), Some("Asia/Kathmandu"));
    let (area, o) = e
        .tz_offset_at_point(p(common::EAST - 10_000_000), when)
        .unwrap()
        .unwrap();
    assert_eq!((area.row, o.seconds), (rows[1], 19_800));
    assert_eq!(e.zone_name(&area).unwrap(), Some("Asia/Kolkata"));
    // Outside every area.
    assert_eq!(
        e.tz_offset_at_point(Point::<E7>::from_units(0, 0).unwrap(), when)
            .unwrap(),
        None
    );
}
