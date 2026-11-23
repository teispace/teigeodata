//! The request core's gates (phase 6.1): every bound of [`Limits`] holds at its edge and refuses
//! past it, every failure has its stable code and never shows a server path, and an answer
//! carries the attribution of the packs that answered it (D-032), not of every pack. An outline
//! steps to a coarser level to stay within its vertex bound and comes out as GeoJSON with its
//! rings wound by RFC 7946; a distance measures between places and points (D-317).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use serde_json::Value;
use teigeodata_core::PlaceKind;
use teigeodata_engine::{Component, Components, Engine, Layer, LayerOptions, Outline, Scratch};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::output::{self, Item, Mode};
use teigeodata_server::query::{
    Ask, At, COARSEST_LOD, Code, Limits, PostalAsk, QueryError, answer, boundary, distance, postal,
};
use teigeodata_server::seams::{Authorize, Bare, Caller, Incoming, Operation, Seams};

/// `pack` again with `notice` as its attribution.
fn noticed(pack: &[u8], notice: &str) -> Vec<u8> {
    let p = Pack::open(pack).unwrap();
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = p
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap();
    w.manifest(Tag::ATTRIBUTION, notice.as_bytes()).unwrap();
    w.pack(&sections, None).unwrap()
}

fn engine(packs: &[Vec<u8>]) -> Engine {
    let mut e = Engine::new();
    for p in packs {
        e.add(Layer::from_bytes(p.clone(), LayerOptions::default()).unwrap());
    }
    e
}

fn run(e: &Engine, ask: &Ask) -> Result<Vec<Item>, QueryError> {
    let mut out = Vec::new();
    answer(e, ask, &Limits::SERVER, &mut Scratch::default(), &mut out)?;
    Ok(out)
}

fn code(r: Result<Vec<Item>, QueryError>) -> Code {
    r.unwrap_err().code
}

fn search(query: &str, limit: usize) -> Ask {
    Ask::Search {
        query: query.into(),
        limit,
        focus: None,
        strict: false,
    }
}

#[test]
fn every_bound_holds_at_its_edge_and_refuses_past_it() {
    let model = common::synthetic(300, 7);
    let e = engine(&[model.encode()]);
    let id = model.ids[0].to_string();
    let l = Limits::SERVER;
    assert_eq!((l.text_bytes, l.limit, l.radius_m), (256, 50, 1_000_000.0));

    // Text: 256 bytes pass, 257 do not, counted in bytes (a two-byte letter).
    assert!(run(&e, &search(&"a".repeat(256), 10)).is_ok());
    assert_eq!(
        code(run(&e, &search(&"a".repeat(257), 10))),
        Code::OutOfBounds
    );
    assert_eq!(
        code(run(&e, &search(&"é".repeat(129), 10))),
        Code::OutOfBounds,
        "258 bytes in 129 letters"
    );

    // Count: 50 pass, 51 do not, for every query that takes one.
    assert!(run(&e, &search("Synthland", 50)).is_ok());
    assert_eq!(code(run(&e, &search("Synthland", 51))), Code::OutOfBounds);
    let reverse = |limit| Ask::Reverse {
        lat: 27.7,
        lon: 85.3,
        limit,
        in_country: false,
    };
    assert!(run(&e, &reverse(50)).is_ok());
    assert_eq!(code(run(&e, &reverse(51))), Code::OutOfBounds);
    let lookup = |n: usize| Ask::Lookup {
        ids: vec![id.clone(); n],
    };
    assert_eq!(run(&e, &lookup(50)).unwrap().len(), 50);
    assert_eq!(code(run(&e, &lookup(51))), Code::OutOfBounds);

    // Radius: (0, 1000 km]; zero, negative, NaN and infinity are refused.
    let near = |m: f64| Ask::Near {
        id: id.clone(),
        limit: 5,
        radius_m: Some(m),
    };
    assert!(run(&e, &near(1_000_000.0)).is_ok());
    for m in [1_000_000.1, 0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(code(run(&e, &near(m))), Code::OutOfBounds, "{m}");
    }

    // A postal code and its scheme are text; a prefix's limit is a count.
    let mut codes = Vec::new();
    let prefix = |code: String, limit| PostalAsk::Code {
        code,
        scheme: None,
        prefix: true,
        limit,
    };
    let p = |ask: &PostalAsk, codes: &mut Vec<_>| {
        postal(&e, ask, &l, &mut Scratch::default(), codes).map_err(|x| x.code)
    };
    // At the bound the engine answers (or says the model has no postal component).
    assert_ne!(
        p(&prefix("4".into(), 50), &mut codes),
        Err(Code::OutOfBounds)
    );
    assert_eq!(
        p(&prefix("4".into(), 51), &mut codes),
        Err(Code::OutOfBounds)
    );
    assert_eq!(
        p(&prefix("4".repeat(257), 5), &mut codes),
        Err(Code::OutOfBounds)
    );

    // No bound for `tgd`.
    let mut out = Vec::new();
    answer(
        &e,
        &search(&"a".repeat(10_000), 1_000),
        &Limits::NONE,
        &mut Scratch::default(),
        &mut out,
    )
    .unwrap();
}

#[test]
fn every_failure_has_its_code_and_hides_server_paths() {
    let model = common::synthetic(300, 7);
    let e = engine(&[model.encode()]);
    let get = |id: &str| Ask::Get {
        id: id.into(),
        names: false,
    };
    assert_eq!(code(run(&e, &get("not an id"))), Code::InvalidArgument);
    let absent = common::synthetic(300, 99).ids[0];
    assert!(!model.ids.contains(&absent));
    let x = run(&e, &get(&absent.to_string())).unwrap_err();
    assert_eq!((x.code, x.code.http_status()), (Code::NotFound, 404));
    assert_eq!(
        code(run(
            &e,
            &Ask::Admin {
                lat: 91.0,
                lon: 0.0
            }
        )),
        Code::InvalidArgument
    );
    assert_eq!(
        code(run(
            &e,
            &Ask::Search {
                query: "x".into(),
                limit: 1,
                focus: Some((27.0, 85.0, 0.0)),
                strict: false,
            }
        )),
        Code::InvalidArgument
    );

    // A pack installed without names, opened from a file: the code says which component, the
    // message never names the file.
    let dir = std::env::temp_dir().join(format!("tgd-server-q-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("secret-place.tgd");
    std::fs::write(&path, model.encode()).unwrap();
    let mut e = Engine::new();
    let points = Components::of(&[Component::Point]).closed();
    e.add(
        Layer::open(
            &path,
            LayerOptions {
                installed: Some(points),
                ..LayerOptions::default()
            },
        )
        .unwrap(),
    );
    let x = run(&e, &search("Synthland", 5)).unwrap_err();
    assert_eq!(
        (x.code, x.code.http_status()),
        (Code::MissingComponent, 501)
    );
    assert_eq!(
        x.message, "the `search` component is not installed",
        "the component, not the layer"
    );
    assert!(!x.message.contains("secret-place"), "{}", x.message);
    assert!(x.cause.is_some(), "the cause stays for the server's log");
    // A mapped pack cannot be removed on Windows.
    drop(e);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_codes_are_stable() {
    let all = [
        (Code::InvalidArgument, "invalid_argument", 400),
        (Code::OutOfBounds, "out_of_bounds", 400),
        (Code::NotFound, "not_found", 404),
        (Code::MissingComponent, "missing_component", 501),
        (Code::Unauthenticated, "unauthenticated", 401),
        (Code::Forbidden, "forbidden", 403),
        (Code::RateLimited, "rate_limited", 429),
        (Code::Unavailable, "unavailable", 503),
        (Code::Internal, "internal", 500),
    ];
    for (c, name, status) in all {
        assert_eq!((c.name(), c.http_status()), (name, status));
    }
}

#[test]
fn a_lookup_keeps_the_order_asked_and_leaves_out_what_is_not_there() {
    let model = common::synthetic(300, 7);
    let e = engine(&[model.encode()]);
    let absent = common::synthetic(300, 99).ids[0].to_string();
    let ids = vec![model.ids[5].to_string(), absent, model.ids[2].to_string()];
    let got: Vec<_> = run(&e, &Ask::Lookup { ids })
        .unwrap()
        .iter()
        .map(|i| i.place.id)
        .collect();
    assert_eq!(got, [model.ids[5], model.ids[2]]);
}

#[test]
fn the_hierarchy_climbs_to_the_root_and_children_come_back_down() {
    let model = common::synthetic(300, 7);
    let e = engine(&[model.encode()]);
    // A row with a parent and a grandparent, so the chain has two links at least.
    let parent = |r: usize| model.parents.get(r).copied().flatten().map(|p| p as usize);
    let row = (0..model.ids.len())
        .find(|&r| parent(r).and_then(parent).is_some())
        .expect("the model has a two-level chain");
    let mut chain = Vec::new();
    let mut at = row;
    while let Some(p) = parent(at) {
        chain.push(model.ids[p]);
        at = p;
    }
    let id = model.ids[row].to_string();
    let got: Vec<_> = run(&e, &Ask::Hierarchy { id: id.clone() })
        .unwrap()
        .iter()
        .map(|i| i.place.id)
        .collect();
    assert_eq!(got, chain, "parent first, to the root");

    let up = model.ids[parent(row).unwrap()].to_string();
    let all = run(
        &e,
        &Ask::Children {
            id: up.clone(),
            limit: 50,
        },
    )
    .unwrap();
    let want = (0..model.ids.len())
        .filter(|&r| parent(r) == parent(row))
        .count();
    assert_eq!(all.len(), want.min(50));
    assert!(all.iter().any(|i| i.place.id == model.ids[row]) || want > 50);
    let one = run(
        &e,
        &Ask::Children {
            id: up.clone(),
            limit: 1,
        },
    )
    .unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(
        code(run(&e, &Ask::Children { id: up, limit: 51 })),
        Code::OutOfBounds
    );
    // A root has no ancestors.
    let root = model.ids[at].to_string();
    assert!(run(&e, &Ask::Hierarchy { id: root }).unwrap().is_empty());
    let absent = common::synthetic(300, 99).ids[0].to_string();
    assert_eq!(
        code(run(&e, &Ask::Hierarchy { id: absent })),
        Code::NotFound
    );
}

#[test]
fn an_answer_carries_the_attribution_of_the_packs_that_answered() {
    let model = common::synthetic(400, 5);
    let parts = model.split(2, |_, id| (id.get() % 2) as usize);
    let e = engine(&[
        noticed(&parts[0], "Source A, ODbL"),
        noticed(&parts[1], "Source B, CC BY 4.0"),
    ]);
    let of = |p: u64| *model.ids.iter().find(|id| id.get() % 2 == p).unwrap();
    let items = run(
        &e,
        &Ask::Get {
            id: of(0).to_string(),
            names: false,
        },
    )
    .unwrap();
    assert_eq!(
        output::attribution_of(&e, &items).as_deref(),
        Some("Source A, ODbL")
    );
    let both = run(
        &e,
        &Ask::Lookup {
            ids: vec![of(1).to_string(), of(0).to_string()],
        },
    )
    .unwrap();
    assert_eq!(
        output::attribution_of(&e, &both).as_deref(),
        Some("Source A, ODbL\nSource B, CC BY 4.0"),
        "each notice once, in the engine's layer order"
    );
    assert_eq!(output::attribution_of(&e, &[]), None, "no place, no notice");

    // The GeoJSON and Nominatim forms carry the same.
    let mut out = Vec::new();
    output::write(&e, &items, Mode::Geojson, &mut out).unwrap();
    let g: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(g["attribution"], "Source A, ODbL");
    out.clear();
    output::write(&e, &both, Mode::Nominatim, &mut out).unwrap();
    let n: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(n[0]["licence"], "Source A, ODbL; Source B, CC BY 4.0");
}

struct Limited;

impl Authorize for Limited {
    fn authorize(&self, caller: &Caller, op: Operation) -> Result<(), QueryError> {
        if op == Operation::Search && caller.id.is_none() {
            return Err(QueryError::new(Code::RateLimited, "too many searches"));
        }
        Ok(())
    }
}

struct Keyed(&'static [u8]);

impl Incoming for Keyed {
    fn header(&self, name: &str) -> Option<&[u8]> {
        name.eq_ignore_ascii_case("x-key").then_some(self.0)
    }
}

#[test]
fn the_seams_are_open_by_default_and_a_deployment_plugs_its_own() {
    let open = Seams::default();
    assert_eq!(
        open.admit(&Bare, Operation::Search).unwrap(),
        Caller::default()
    );
    let ask = search("x", 1);
    assert_eq!(Operation::of(&ask).name(), "search");

    let limited = Seams {
        authorize: std::sync::Arc::new(Limited),
        ..Seams::default()
    };
    assert_eq!(
        limited
            .admit(&Keyed(b"k"), Operation::Search)
            .unwrap_err()
            .code,
        Code::RateLimited
    );
    assert!(limited.admit(&Bare, Operation::Get).is_ok());
}

/// Twice the signed area of a GeoJSON ring (shoelace, x = lon): positive counterclockwise.
fn winding(ring: &Value) -> f64 {
    let pts: Vec<(f64, f64)> = ring
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect();
    pts.windows(2)
        .map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1)
        .sum()
}

#[test]
fn an_outline_steps_coarser_to_its_vertex_bound_and_is_wound_by_rfc_7946() {
    let (model, _) = common::outlined(false);
    let e = engine(&[model.encode()]);
    let row = model
        .kinds
        .iter()
        .position(|&k| k == PlaceKind::Country)
        .unwrap();
    let id = model.ids[row].to_string();
    // The country: an outer ring and a hole, 6 vertices each at full resolution and 5 at
    // every coarser level (the mid-edge point goes).
    let bound = |vertices| Limits {
        vertices,
        ..Limits::SERVER
    };
    let mut out = Outline::default();
    let got = |lod, vertices, out: &mut Outline| boundary(&e, &id, lod, &bound(vertices), out);
    let s = got(None, 12, &mut out).unwrap();
    assert_eq!((s.lod, s.outlined, out.points.len()), (0, true, 12));
    let s = got(None, 11, &mut out).unwrap();
    assert_eq!((s.lod, out.points.len()), (1, 10), "one level coarser");
    let s = got(Some(4), 12, &mut out).unwrap();
    assert_eq!(s.lod, 4, "the level asked when within the bound");
    let s = got(Some(COARSEST_LOD), 10, &mut out).unwrap();
    assert_eq!(s.lod, COARSEST_LOD);
    let past = got(None, 9, &mut out).unwrap_err();
    assert_eq!(past.code, Code::OutOfBounds);
    assert_eq!(
        past.message,
        format!("{id}: the outline has 10 vertices at its coarsest, past the most allowed, 9")
    );
    let past = got(Some(COARSEST_LOD + 1), 12, &mut out).unwrap_err();
    assert_eq!(
        (past.code, past.message.as_str()),
        (Code::InvalidArgument, "`lod` must be 0 to 6")
    );
    let absent = common::synthetic(300, 99).ids[0].to_string();
    assert_eq!(
        boundary(&e, &absent, None, &Limits::SERVER, &mut out)
            .unwrap_err()
            .code,
        Code::NotFound
    );

    // As GeoJSON: the outer ring counterclockwise, the hole clockwise, closed, with what it is.
    let s = got(Some(0), 12, &mut out).unwrap();
    let mut body = Vec::new();
    output::outline(&e, &s.place, &out, s.lod, s.outlined, &mut body).unwrap();
    let f: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(f["type"], "Feature");
    assert_eq!(f["id"], id.as_str());
    assert_eq!(f["geometry"]["type"], "Polygon", "{f}");
    let rings = f["geometry"]["coordinates"].as_array().unwrap();
    assert_eq!(rings.len(), 2);
    assert!(winding(&rings[0]) > 0.0, "outer counterclockwise: {f}");
    assert!(winding(&rings[1]) < 0.0, "hole clockwise: {f}");
    for r in rings {
        let r = r.as_array().unwrap();
        assert_eq!(r.first(), r.last(), "closed");
    }
    assert_eq!(f["properties"]["lod"], 0);
    assert_eq!(f["properties"]["vertices"], 12);
    assert_eq!(f["properties"]["tolerance_m"], 0.0);

    // A settlement has no outline: its point.
    let r = model
        .kinds
        .iter()
        .position(|&k| k == PlaceKind::Settlement)
        .unwrap();
    let town = model.ids[r].to_string();
    let s = boundary(&e, &town, None, &Limits::SERVER, &mut out).unwrap();
    assert!(!s.outlined);
    let mut body = Vec::new();
    output::outline(&e, &s.place, &out, s.lod, s.outlined, &mut body).unwrap();
    let f: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(f["geometry"]["type"], "Point", "{f}");
}

#[test]
fn a_distance_measures_between_places_and_points() {
    let model = common::synthetic(300, 7);
    let e = engine(&[model.encode()]);
    let at = |lat, lon| At::Point { lat, lon };
    // A degree of the equator on the mean sphere (R = 6,371,008.8 m): 111,195.08 m, east.
    let m = distance(&e, &at(0.0, 0.0), &at(0.0, 1.0), &Limits::SERVER).unwrap();
    assert!((m.meters - 111_195.08).abs() < 0.01, "{}", m.meters);
    assert!((m.bearing_deg - 90.0).abs() < 1e-9, "{}", m.bearing_deg);
    let m = distance(&e, &at(1.0, 0.0), &at(0.0, 0.0), &Limits::SERVER).unwrap();
    assert!(
        (m.bearing_deg - 180.0).abs() < 1e-9,
        "south: {}",
        m.bearing_deg
    );
    // A place's end is its point, and carries the place.
    let id = model.ids[0].to_string();
    let p = model.points[0];
    let (lat, lon) = (p.lat_degrees(), p.lon_degrees());
    let m = distance(&e, &At::parse(&id), &at(lat, lon), &Limits::SERVER).unwrap();
    assert!(m.meters < 1e-6, "{}", m.meters);
    assert_eq!(m.from.place.map(|p| p.id), Some(model.ids[0]));
    assert!(m.to.place.is_none());
    assert_eq!(At::parse(" 27.7 , 85.3 "), at(27.7, 85.3));
    assert_eq!(At::parse("tgd:np/1xhb"), At::Place("tgd:np/1xhb".into()));
    // Failures: a place not there, a point off the globe.
    let absent = common::synthetic(300, 99).ids[0].to_string();
    assert_eq!(
        distance(&e, &At::parse(&absent), &at(0.0, 0.0), &Limits::SERVER)
            .unwrap_err()
            .code,
        Code::NotFound
    );
    assert_eq!(
        distance(&e, &at(91.0, 0.0), &at(0.0, 0.0), &Limits::SERVER)
            .unwrap_err()
            .code,
        Code::InvalidArgument
    );
}

/// A place's class (D-381) is in every writer's output, though the public `Record` keeps its
/// 1.0 fields (D-388).
#[test]
fn a_class_reaches_json_geojson_and_text() {
    let mut model = common::synthetic(20, 3);
    let temple = teigeodata_core::class::class_code("temple").unwrap();
    model.classes = (0..model.ids.len())
        .map(|r| if r == 0 { temple } else { 0 })
        .collect();
    let e = engine(&[model.encode()]);
    let place = e.get(model.ids[0]).unwrap().unwrap();
    let items = [Item::from(place)];
    let write = |mode| {
        let mut out = Vec::new();
        output::write(&e, &items, mode, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let json: Value = serde_json::from_str(&write(Mode::Json)).unwrap();
    assert_eq!(json[0]["class"], "temple", "{json}");
    assert_eq!(json[0]["id"], model.ids[0].to_string());
    let geo: Value = serde_json::from_str(&write(Mode::Geojson)).unwrap();
    assert_eq!(geo["features"][0]["properties"]["class"], "temple", "{geo}");
    assert!(write(Mode::Text).contains(" temple"));
    // A place without one has no field.
    let other = [Item::from(e.get(model.ids[1]).unwrap().unwrap())];
    let mut out = Vec::new();
    output::write(&e, &other, Mode::Json, &mut out).unwrap();
    let json: Value = serde_json::from_slice(&out).unwrap();
    assert!(json[0].get("class").is_none(), "{json}");
}
