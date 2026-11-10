#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::{Value, json};
use teigeodata_core::Point;
use teigeodata_format::Bbox;
use teigeodata_format::component::{Component, Components};

use super::{Entry, Error, Size, file_name, path};
use crate::trust::{Target, canonical_json};

const ASSET_LEN: u64 = 8_227_234;
const BLAKE3: [u8; 32] = [0x1a; 32];

fn entry() -> Entry {
    let mut sizes = BTreeMap::new();
    sizes.insert(
        Component::Point,
        Size {
            wire: 522_192,
            disk: 601_344,
        },
    );
    sizes.insert(
        Component::Names,
        Size {
            wire: 917_602,
            disk: 1_302_528,
        },
    );
    sizes.insert(
        Component::Search,
        Size {
            wire: 300_000,
            disk: 410_000,
        },
    );
    sizes.insert(
        Component::Elevation,
        Size {
            wire: 90_000,
            disk: 200_000,
        },
    );
    Entry {
        address: "np".to_string(),
        component: "places".to_string(),
        release: "2026.11.0".to_string(),
        file: file_name("np", "places", "2026.11.0", &BLAKE3),
        pack_length: 11_748_636,
        head: [0xab; 16],
        seek_table: 293,
        sizes,
        deps: alloc::vec!["np.baseline.tgd".to_string()],
        min_reader: 0,
        bbox: Some(Bbox {
            min: Point::from_units(263_000_000, 801_000_000).unwrap(),
            max: Point::from_units(304_500_000, 882_000_000).unwrap(),
        }),
        licence: "ODbL-1.0".to_string(),
    }
}

fn target(custom: Value) -> Target {
    let mut hashes = BTreeMap::new();
    hashes.insert("blake3".to_string(), BLAKE3.to_vec());
    Target {
        length: ASSET_LEN,
        hashes,
        custom: Some(custom),
    }
}

/// The entry's custom object with `edit` applied.
fn with(edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Target {
    let mut v = entry().to_custom();
    edit(v.as_object_mut().unwrap());
    target(v)
}

#[test]
fn an_entry_round_trips_through_its_custom_object() {
    let e = entry();
    assert_eq!(e.path(), "np.places.tgd");
    assert_eq!(e.file, "np.places.2026.11.0.1a1a1a1a.tgd.zst");
    let back = Entry::from_target(&e.path(), &target(e.to_custom())).unwrap();
    assert_eq!(back, e);
    // Canonical JSON has no floats: the whole object encodes.
    let bytes = canonical_json(&e.to_custom()).unwrap();
    // 466 B on 2026-11-10 (D-160); a growing entry grows the catalog of every app.
    assert!(bytes.len() <= 480, "{} B", bytes.len());
}

#[test]
fn unknown_fields_and_components_are_skipped() {
    let t = with(|m| {
        m.insert("future".to_string(), json!({"x": 1}));
        m["sizes"]["hologram"] = json!([1, 2]);
    });
    assert_eq!(Entry::from_target("np.places.tgd", &t).unwrap(), entry());
}

#[test]
fn a_plan_counts_what_a_set_needs() {
    let e = entry();
    // Search needs names, and names needs point.
    let s = e.size_of(Components::of(&[Component::Search]));
    assert_eq!(
        s,
        Size {
            wire: 522_192 + 917_602 + 300_000,
            disk: 601_344 + 1_302_528 + 410_000
        }
    );
    let p = e.size_of(Components::of(&[Component::Point]));
    assert_eq!(
        p,
        Size {
            wire: 522_192,
            disk: 601_344
        }
    );
    // A component the pack lacks counts nothing.
    assert_eq!(e.size_of(Components::of(&[Component::Dem])).wire, 0);
}

/// The entry with `key` set to `v` (`key` may be `outer.inner`).
fn set(key: &str, v: Value) -> Target {
    with(|m| match key.split_once('.') {
        Some((outer, inner)) => m[outer][inner] = v,
        None => {
            m.insert(key.to_string(), v);
        }
    })
}

#[test]
#[allow(clippy::too_many_lines, reason = "one row per case")]
fn every_malformed_or_disagreeing_entry_is_refused() {
    let file = "file is not address.component.release.hash8.tgd.zst";
    let cases: Vec<(&str, Target, Error)> = alloc::vec![
        ("schema 2", set("schema", json!(2)), Error::Schema(2)),
        (
            "no schema",
            with(|m| drop(m.remove("schema"))),
            Error::Field("schema")
        ),
        (
            "upper-case address",
            set("address", json!("NP")),
            Error::Field("address")
        ),
        (
            "release with a slash",
            set("release", json!("2026/11")),
            Error::Field("release")
        ),
        (
            "another asset",
            set("file", json!("np.places.2026.11.0.00000000.tgd.zst")),
            Error::Name(file)
        ),
        (
            "short head",
            set("pack.head", json!("abab")),
            Error::Field("pack.head")
        ),
        (
            "upper-case head",
            set("pack.head", json!("AB".repeat(16))),
            Error::Field("pack.head")
        ),
        (
            "raw",
            set("transport.encoding", json!("raw")),
            Error::Field("transport.encoding")
        ),
        (
            "no seek table",
            set("transport.seek_table", json!(0)),
            Error::Field("transport.seek_table")
        ),
        (
            "all seek table",
            set("transport.seek_table", json!(ASSET_LEN)),
            Error::Field("transport.seek_table")
        ),
        ("no sizes", set("sizes", json!({})), Error::Field("sizes")),
        (
            "one number",
            set("sizes.point", json!([1])),
            Error::Field("sizes")
        ),
        (
            "wire > asset",
            set("sizes.point", json!([ASSET_LEN + 1, 1])),
            Error::Field("sizes")
        ),
        (
            "disk > pack",
            set("sizes.point", json!([1, 11_748_637])),
            Error::Field("sizes")
        ),
        (
            "itself",
            set("deps", json!(["np.places.tgd"])),
            Error::Field("deps")
        ),
        (
            "twice",
            set("deps", json!(["np.dem.tgd", "np.dem.tgd"])),
            Error::Field("deps")
        ),
        (
            "not a path",
            set("deps", json!(["np"])),
            Error::Field("deps")
        ),
        (
            "past u16",
            set("min_reader", json!(70_000)),
            Error::Field("min_reader")
        ),
        (
            "floats",
            set("bbox", json!([80.1, 26.3, 88.2, 30.4])),
            Error::Field("bbox")
        ),
        (
            "lat > 90",
            set("bbox", json!([0, 0, 1, 900_000_001])),
            Error::Field("bbox")
        ),
        (
            "south > north",
            set("bbox", json!([0, 10, 1, 5])),
            Error::Field("bbox")
        ),
        (
            "a space",
            set("licence", json!("CC BY")),
            Error::Field("licence")
        ),
    ];
    for (why, t, want) in cases {
        assert_eq!(Entry::from_target("np.places.tgd", &t), Err(want), "{why}");
    }
    let e = entry();
    assert_eq!(
        Entry::from_target("np.dem.tgd", &target(e.to_custom())),
        Err(Error::Name("the path is not address.component.tgd"))
    );
    let mut t = target(e.to_custom());
    t.hashes.clear();
    t.hashes.insert("sha256".to_string(), alloc::vec![0; 32]);
    assert_eq!(
        Entry::from_target("np.places.tgd", &t),
        Err(Error::Name("the target has no blake3"))
    );
    t.custom = None;
    assert_eq!(Entry::from_target("np.places.tgd", &t), Err(Error::Missing));
}

#[test]
fn a_box_across_the_antimeridian_is_kept() {
    let t = with(|m| {
        m.insert(
            "bbox".into(),
            json!([1_790_000_000, -200_000_000, -1_790_000_000, -100_000_000]),
        );
    });
    let b = Entry::from_target("np.places.tgd", &t)
        .unwrap()
        .bbox
        .unwrap();
    assert!(b.max.lon() < b.min.lon());
}

/// The checked-in JSON schema names the same fields this reader requires and writes.
#[test]
fn the_json_schema_matches_the_reader() {
    let schema: Value =
        serde_json::from_str(include_str!("../../schema/catalog-custom-v1.json")).unwrap();
    let names = |v: &Value| -> Vec<String> {
        let mut out: Vec<String> = match v {
            Value::Array(a) => a.iter().map(|x| x.as_str().unwrap().to_string()).collect(),
            Value::Object(m) => m.keys().cloned().collect(),
            _ => panic!("{v}"),
        };
        out.sort();
        out
    };
    let written = entry().to_custom();
    assert_eq!(names(&schema["properties"]), names(&written));
    let mut no_bbox = written.clone();
    no_bbox.as_object_mut().unwrap().remove("bbox");
    // Every field but the optional bbox is required; removing any one is refused.
    assert_eq!(names(&schema["required"]), names(&no_bbox));
    for k in names(&no_bbox) {
        let t = with(|m| {
            m.remove(&k);
        });
        assert!(Entry::from_target("np.places.tgd", &t).is_err(), "{k}");
    }
    for (k, sub) in [("pack", "pack"), ("transport", "transport")] {
        assert_eq!(
            names(&schema["properties"][k]["required"]),
            names(&written[sub]),
            "{k}"
        );
    }
    assert_eq!(
        schema["properties"]["schema"]["const"],
        json!(super::SCHEMA)
    );
    assert_eq!(
        schema["properties"]["transport"]["properties"]["encoding"]["const"],
        json!(super::ENCODING)
    );
}

#[test]
fn names_are_built_one_way() {
    assert_eq!(path("in-br-t07", "places"), "in-br-t07.places.tgd");
    assert_eq!(
        file_name("ae-du", "dem", "2026.12.1", &[0, 0xff, 0x10, 0x01, 9, 9]),
        "ae-du.dem.2026.12.1.00ff1001.tgd.zst"
    );
}
