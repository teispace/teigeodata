//! The request core's gates (phase 6.1): every bound of [`Limits`] holds at its edge and refuses
//! past it, every failure has its stable code and never shows a server path, and an answer
//! carries the attribution of the packs that answered it (D-032), not of every pack.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use teigeodata_engine::{Component, Components, Engine, Layer, LayerOptions, Scratch};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_server::output::{self, Item, Mode};
use teigeodata_server::query::{Ask, Code, Limits, PostalAsk, QueryError, answer, postal};
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
    let g: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(g["attribution"], "Source A, ODbL");
    out.clear();
    output::write(&e, &both, Mode::Nominatim, &mut out).unwrap();
    let n: serde_json::Value = serde_json::from_slice(&out).unwrap();
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
