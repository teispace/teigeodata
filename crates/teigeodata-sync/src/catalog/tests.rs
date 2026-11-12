#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::{Value, json};
use teigeodata_core::Point;
use teigeodata_format::component::{Component, Components, Parts, Selection};
use teigeodata_format::{Bbox, Part};

use super::{BlockHashes, Entry, Error, PartSize, Size, file_name, path};
use crate::trust::{Target, canonical_json};

const ASSET_LEN: u64 = 8_227_234;
const SHA256: [u8; 32] = [0x1a; 32];

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
        file: file_name("np", "places", "2026.11.0", &SHA256),
        pack_length: 11_748_636,
        head: [0xab; 16],
        seek_table: 293,
        head_frame: Size {
            wire: 512,
            disk: 1_024,
        },
        block_hashes: Some(BlockHashes {
            size: Size {
                wire: 6_100,
                disk: 6_144,
            },
            needed_by: Components::of(&[Component::Names]),
        }),
        sizes,
        parts: Vec::new(),
        deps: alloc::vec!["np.baseline.tgd".to_string()],
        min_reader: 0,
        bbox: Some(Bbox {
            min: Point::from_units(263_000_000, 801_000_000).unwrap(),
            max: Point::from_units(304_500_000, 882_000_000).unwrap(),
        }),
        licence: "ODbL-1.0".to_string(),
    }
}

/// [`entry`] with its NAMES in three parts (base, `*`, `ne`) and a boundaries component with a
/// coarser outline part.
fn entry_with_parts() -> Entry {
    let mut e = entry();
    let size = |wire, disk| Size { wire, disk };
    e.sizes
        .insert(Component::Boundaries, size(400_000, 500_000));
    e.parts = alloc::vec![
        PartSize {
            component: Component::Names,
            part: Part::REST,
            size: size(200_000, 280_000),
            tolerance_m: 0,
        },
        PartSize {
            component: Component::Names,
            part: Part::language("ne").unwrap(),
            size: size(400_000, 560_000),
            tolerance_m: 0,
        },
        PartSize {
            component: Component::Boundaries,
            part: Part::outline(2, 3).unwrap(),
            size: size(30_000, 40_000),
            tolerance_m: 100,
        },
    ];
    e.parts.sort_unstable_by_key(|p| (p.component, p.part));
    e
}

#[test]
fn parts_round_trip_and_a_selection_counts_only_its_parts() {
    let e = entry_with_parts();
    let back = Entry::from_target(&e.path(), &target(e.to_custom())).unwrap();
    assert_eq!(back, e);
    let names = Components::of(&[Component::Names]);
    let whole = e.size_of(names);
    // English: the base and `*` (English has no part of its own here), not `ne`.
    let en = Selection {
        set: names,
        parts: Parts::ALL
            .languages(&[Part::language("en").unwrap()])
            .unwrap(),
    };
    assert_eq!(
        e.resolve(en.parts).language_parts(),
        Some(&[Part::REST][..])
    );
    let s = e.size_of(en);
    assert_eq!(
        (whole.wire - s.wire, whole.disk - s.disk),
        (400_000, 560_000)
    );
    // One outline part: the boundaries' base (400,000 − 30,000 on the wire) is not fetched.
    let b = Components::of(&[Component::Boundaries]);
    let one = Selection {
        set: b,
        parts: Parts::ALL.outline(Part::outline(2, 3).unwrap()),
    };
    assert_eq!(e.size_of(b).wire - e.size_of(one).wire, 370_000);
    // The base outline part alone: everything but the listed part.
    let base = Selection {
        set: b,
        parts: Parts::ALL.outline(Part::BASE),
    };
    assert_eq!(e.size_of(b).wire - e.size_of(base).wire, 30_000);
    // A bad part key is refused.
    let t = target({
        let mut c = e.to_custom();
        c["parts"]["names"][0][0] = json!(0);
        c
    });
    assert!(Entry::from_target(&e.path(), &t).is_err());
}

fn target(custom: Value) -> Target {
    let mut hashes = BTreeMap::new();
    hashes.insert("sha256".to_string(), SHA256.to_vec());
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
    // 536 B on 2026-11-10 with BLOCK_HASHES (D-162; 466 B before, D-160); a growing entry
    // grows the catalog of every app.
    assert!(bytes.len() <= 550, "{} B", bytes.len());
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
    let fixed = Size {
        wire: 512 + 293,
        disk: 1_024 + 32,
    };
    let plus = |a: Size, w: u64, d: u64| Size {
        wire: a.wire + w,
        disk: a.disk + d,
    };
    // Search needs names, and names needs point; names needs BLOCK_HASHES.
    let s = e.size_of(Components::of(&[Component::Search]));
    let own = plus(
        fixed,
        522_192 + 917_602 + 300_000,
        601_344 + 1_302_528 + 410_000,
    );
    assert_eq!(s, plus(own, 6_100, 6_144));
    // Point alone: no BLOCK_HASHES.
    let p = e.size_of(Components::of(&[Component::Point]));
    assert_eq!(p, plus(fixed, 522_192, 601_344));
    // A component the pack lacks counts nothing beyond what every install fetches.
    assert_eq!(e.size_of(Components::of(&[Component::Dem])), fixed);
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
    t.hashes.insert("sha512".to_string(), alloc::vec![0; 64]);
    assert_eq!(
        Entry::from_target("np.places.tgd", &t),
        Err(Error::Name("the target has no sha256"))
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
    let written = entry_with_parts().to_custom();
    assert_eq!(names(&schema["properties"]), names(&written));
    let mut no_bbox = written.clone();
    no_bbox.as_object_mut().unwrap().remove("bbox");
    no_bbox.as_object_mut().unwrap().remove("block_hashes");
    no_bbox.as_object_mut().unwrap().remove("parts");
    // Every field but the optional bbox, block_hashes and parts is required; removing any one
    // is refused.
    assert_eq!(names(&schema["required"]), names(&no_bbox));
    for k in names(&no_bbox) {
        let t = with(|m| {
            m.remove(&k);
        });
        assert!(Entry::from_target("np.places.tgd", &t).is_err(), "{k}");
    }
    for (k, sub) in [
        ("pack", "pack"),
        ("transport", "transport"),
        ("block_hashes", "block_hashes"),
    ] {
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

mod find {
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec::Vec;

    use teigeodata_core::{E7, Point};
    use teigeodata_format::Bbox;

    use super::{SHA256, entry, target};
    use crate::catalog::{Candidate, Catalog, Error, Resolver, file_name};
    use crate::trust::{Common, Target, Targets};

    fn pt(lat: f64, lon: f64) -> Point<E7> {
        Point::from_degrees(lat, lon).unwrap()
    }

    fn pack(address: &str, bbox: Option<Bbox>) -> (alloc::string::String, Target) {
        let mut e = entry();
        e.address = address.to_string();
        e.file = file_name(address, "places", "2026.11.0", &SHA256);
        e.bbox = bbox;
        (e.path(), target(e.to_custom()))
    }

    fn catalog() -> (Catalog, Vec<(alloc::string::String, Error)>) {
        let dubai = Bbox {
            min: pt(24.6, 54.8),
            max: pt(25.4, 56.0),
        };
        let abu_dhabi = Bbox {
            min: pt(22.6, 51.5),
            max: pt(24.9, 56.0),
        };
        let mut targets = BTreeMap::new();
        for (a, b) in [
            ("np", None),
            ("in", None),
            ("ae", None),
            ("ae-du", Some(dubai)),
            ("ae-az", Some(abu_dhabi)),
            ("ae-sh", None),
        ] {
            let (p, t) = pack(a, b);
            targets.insert(p, t);
        }
        // An entry this reader refuses (a newer schema) is reported, not fatal.
        let (p, mut t) = pack("om", None);
        t.custom.as_mut().unwrap()["schema"] = serde_json::json!(2);
        targets.insert(p, t);
        Catalog::new(&Targets {
            common: Common {
                version: 1,
                expires: 0,
            },
            targets,
            delegations: None,
        })
    }

    /// A resolver that knows two names.
    struct Fixed;

    impl Resolver for Fixed {
        type Error = ();

        fn resolve(&self, query: &str, k: usize, out: &mut Vec<Candidate>) -> Result<(), ()> {
            out.clear();
            let c = |id, p, cc: &[[u8; 2]]| Candidate {
                id,
                point: p,
                score: 1,
                countries: cc.to_vec(),
            };
            match query {
                "Dubai" => out.extend([c(1, pt(25.2, 55.3), &[*b"ae", *b"om"])]),
                "Birgunj" => out.extend([
                    c(2, pt(27.0, 84.87), &[*b"np", *b"in"]),
                    c(3, pt(1.0, 1.0), &[]),
                ]),
                "error" => return Err(()),
                _ => {}
            }
            out.truncate(k);
            Ok(())
        }
    }

    fn paths(c: &Catalog, q: &str, k: usize) -> Vec<Vec<alloc::string::String>> {
        c.find(&Fixed, q, k)
            .unwrap()
            .iter()
            .map(|f| f.packs.iter().map(|e| e.path()).collect())
            .collect()
    }

    #[test]
    fn a_name_finds_its_country_and_the_parts_whose_box_holds_it() {
        let (c, refused) = catalog();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].1, Error::Schema(2));
        assert_eq!(c.entries().len(), 6);
        // Dubai: the country, Dubai's own pack and a part without a box; not Abu Dhabi,
        // whose box ends south of it; Oman has no accepted pack.
        assert_eq!(
            paths(&c, "Dubai", 5),
            [["ae-du.places.tgd", "ae-sh.places.tgd", "ae.places.tgd"]]
        );
        // A border town: both countries' packs; a candidate without countries: none.
        assert_eq!(
            paths(&c, "Birgunj", 5),
            [alloc::vec!["np.places.tgd", "in.places.tgd"], alloc::vec![]]
        );
        assert_eq!(paths(&c, "Birgunj", 1).len(), 1);
        assert_eq!(
            paths(&c, "Nowhere", 5),
            Vec::<Vec<alloc::string::String>>::new()
        );
        assert_eq!(c.find(&Fixed, "error", 5), Err(()));
    }

    #[test]
    fn boxes_across_the_antimeridian_hold_both_sides() {
        let fiji = Bbox {
            min: pt(-21.0, 176.0),
            max: pt(-12.0, -178.0),
        };
        let mut targets = BTreeMap::new();
        let (p, t) = pack("fj-w", Some(fiji));
        targets.insert(p, t);
        let (c, _) = Catalog::new(&Targets {
            common: Common {
                version: 1,
                expires: 0,
            },
            targets,
            delegations: None,
        });
        assert_eq!(c.packs_at(*b"fj", pt(-17.0, 178.0)).count(), 1);
        assert_eq!(c.packs_at(*b"fj", pt(-17.0, -179.0)).count(), 1);
        assert_eq!(c.packs_at(*b"fj", pt(-17.0, 170.0)).count(), 0);
        assert_eq!(c.packs_at(*b"fj", pt(-25.0, 178.0)).count(), 0);
    }
}
