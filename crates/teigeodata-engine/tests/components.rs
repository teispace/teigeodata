//! Components (D-133, D-135): a layer installed with only some components never reads the
//! others (their bytes are zeroed here, as holes in a sparse install), answers every query its
//! components serve exactly as the whole pack does, and refuses every other one with
//! `MissingComponent` naming the component, never a wrong or empty answer.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    reason = "tests: failures panic with their message; indexes are drawn below a slice length"
)]

mod common;

use common::{Rng, synthetic};
use teigeodata_core::{E7, PlaceId, Point};
use teigeodata_engine::{
    Component, Components, Engine, Error, Layer, LayerOptions, Place, Rect, Scratch, Verify,
};
use teigeodata_format::blocks::MIN_COVERED;
use teigeodata_format::tz_rules;
use teigeodata_format::{Encoding, Kind, Pack, Writer};

/// `pack` with a 2 MiB BLOBS section (`extra`) added, so it has `BLOCK_HASHES`.
fn with_blobs(pack: &[u8]) -> Vec<u8> {
    let p = Pack::open(pack).unwrap();
    let mut x = 0x2545_f491_u32;
    let blob: Vec<u8> = (0..2 << 20)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.to_le_bytes()[0]
        })
        .collect();
    let mut sections: Vec<(Kind, Encoding, u64, &[u8])> = p
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    sections.push((Kind::BLOBS, Encoding::RAW, 1, &blob));
    let m = p.manifest();
    let mut w = Writer::new(m.address(), m.component(), m.release()).unwrap();
    if let Some(b) = m.get(teigeodata_format::Tag::BBOX) {
        w.manifest(teigeodata_format::Tag::BBOX, b).unwrap();
    }
    w.pack(&sections, None).unwrap()
}

/// `bytes` with every section outside `installed` zeroed: a hole, as storage leaves it.
/// `BLOCK_HASHES` too, unless an installed section is large enough to need it.
fn holed(bytes: &[u8], installed: Components) -> Vec<u8> {
    let pack = Pack::open(bytes).unwrap();
    let mut out = bytes.to_vec();
    let base = bytes.as_ptr() as usize;
    let inside = |k: Kind| Component::of(k).is_some_and(|c| installed.contains(c));
    let blocked = pack
        .sections()
        .any(|s| inside(s.kind()) && s.bytes().len() as u64 >= MIN_COVERED);
    for s in pack.sections() {
        let hole = match Component::of(s.kind()) {
            Some(c) => !installed.contains(c),
            None => s.kind() == Kind::BLOCK_HASHES && !blocked,
        };
        if hole {
            let start = s.bytes().as_ptr() as usize - base;
            out[start..start + s.bytes().len()].fill(0);
        }
    }
    out
}

fn open(bytes: Vec<u8>, installed: Option<Components>, verify: Verify) -> Engine {
    let mut e = Engine::new();
    let options = LayerOptions {
        installed,
        verify,
        ..LayerOptions::default()
    };
    e.add(Layer::from_bytes(bytes, options).unwrap());
    e
}

/// What one query answered: a value or the missing component.
type Answer<T> = Result<T, Component>;

fn answer<T>(r: Result<T, Error>) -> Answer<T> {
    r.map_err(|e| match e {
        Error::MissingComponent { component, .. } => component,
        e => panic!("{e}"),
    })
}

#[derive(Debug, PartialEq)]
struct Answers {
    get: Vec<Option<u64>>,
    reverse: Vec<Vec<u64>>,
    boxes: Vec<Vec<u64>>,
    elevation: Vec<Answer<Option<i16>>>,
    name: Vec<Answer<Option<String>>>,
    tz_of: Vec<Answer<Option<u64>>>,
    tz_at: Vec<Answer<Option<u64>>>,
    admin_at: Vec<Answer<Vec<u64>>>,
    ancestors: Vec<Answer<Vec<u64>>>,
    search: Vec<Answer<Vec<u64>>>,
    context: Answer<Vec<u64>>,
}

fn ids(places: &[Place]) -> Vec<u64> {
    places.iter().map(|p| p.id.get()).collect()
}

fn answers(e: &Engine, probe: &[PlaceId], points: &[Point<E7>], queries: &[String]) -> Answers {
    let mut s = Scratch::default();
    let (mut near, mut out, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    let places: Vec<Place> = probe.iter().filter_map(|&id| e.get(id).unwrap()).collect();
    let mut buf = [0_u8; 1024];
    let mut a = Answers {
        get: probe
            .iter()
            .map(|&id| e.get(id).unwrap().map(|p| p.id.get()))
            .collect(),
        reverse: Vec::new(),
        boxes: Vec::new(),
        elevation: places.iter().map(|p| answer(e.elevation(p))).collect(),
        name: places
            .iter()
            .map(|p| answer(e.name(p, &mut buf).map(|n| n.map(str::to_owned))))
            .collect(),
        tz_of: places
            .iter()
            .map(|p| answer(e.tz_of(p).map(|z| z.map(|z| z.id.get()))))
            .collect(),
        tz_at: Vec::new(),
        admin_at: Vec::new(),
        ancestors: places
            .iter()
            .map(|p| answer(e.ancestors(p, &mut out).map(|()| ids(&out))))
            .collect(),
        search: Vec::new(),
        context: Ok(Vec::new()),
    };
    for &at in points {
        e.reverse(at.to_e6(), 5, &mut s, &mut near).unwrap();
        a.reverse
            .push(near.iter().map(|n| n.place.id.get()).collect());
        let c = at.to_e6();
        let rect = Rect {
            min_lat: c.lat() - 50_000,
            max_lat: c.lat() + 50_000,
            min_lon: c.lon() - 50_000,
            max_lon: c.lon() + 50_000,
        };
        e.within_bbox(rect, 50, &mut s, &mut out).unwrap();
        a.boxes.push(ids(&out));
        a.tz_at
            .push(answer(e.tz_at(at).map(|z| z.map(|z| z.id.get()))));
        a.admin_at
            .push(answer(e.admin_at(at, &mut s, &mut out).map(|_| ids(&out))));
    }
    for q in queries {
        a.search.push(answer(
            e.search(q, 5, &mut s, &mut hits)
                .map(|()| hits.iter().map(|h| h.place.id.get()).collect()),
        ));
    }
    let context = format!("{}, {}", queries[0], queries[1]);
    a.context = answer(
        e.search(&context, 5, &mut s, &mut hits)
            .map(|()| hits.iter().map(|h| h.place.id.get()).collect()),
    );
    a
}

/// What a query answers with `set` installed: the whole pack's answer if `set` holds one of
/// the components that serve it, else the component it names.
fn expect<T: Clone>(
    full: &[Answer<T>],
    set: Components,
    serve: &[Component],
    name: Component,
) -> Vec<Answer<T>> {
    full.iter()
        .map(|a| {
            if serve.iter().any(|&c| set.contains(c)) {
                a.clone()
            } else {
                Err(name)
            }
        })
        .collect()
}

#[test]
fn each_component_set_answers_as_the_whole_pack_or_names_what_is_missing() {
    use Component::{
        Admin, AdminIndex, Elevation, Names, Point as P, Search, Timezone, TimezoneIndex,
    };
    let model = synthetic(800, 21);
    let whole = with_blobs(&model.encode());
    let mut r = Rng(9);
    let probe: Vec<PlaceId> = (0..60)
        .map(|_| model.ids[r.below(model.ids.len() as u64) as usize])
        .collect();
    let points: Vec<Point<E7>> = (0..40)
        .map(|_| model.points[r.below(model.points.len() as u64) as usize].to_e7())
        .collect();
    let mut buf = [0_u8; 1024];
    let full_engine = open(whole.clone(), None, Verify::Eager);
    let queries: Vec<String> = probe
        .iter()
        .take(8)
        .map(|&id| {
            let p = full_engine.get(id).unwrap().unwrap();
            full_engine.name(&p, &mut buf).unwrap().unwrap().to_owned()
        })
        .collect();
    let full = answers(&full_engine, &probe, &points, &queries);
    let has = full_engine.layers().next().unwrap().components();
    assert_eq!(
        has,
        Components::all().without(Components::of(&[
            Component::Boundaries,
            Component::Postal,
            Component::Dem,
            Component::TzRules,
            Component::Substring,
            Component::DemCoarse,
        ]))
    );
    // The probes are not trivially empty.
    assert!(full.elevation.iter().any(|a| matches!(a, Ok(Some(_)))));
    assert!(full.tz_of.iter().any(|a| matches!(a, Ok(Some(_)))));
    assert!(
        full.admin_at
            .iter()
            .any(|a| a.as_ref().is_ok_and(|v| !v.is_empty()))
    );
    assert!(
        full.ancestors
            .iter()
            .any(|a| a.as_ref().is_ok_and(|v| v.len() > 1))
    );
    assert!(
        full.search
            .iter()
            .all(|a| a.as_ref().is_ok_and(|v| !v.is_empty()))
    );

    let sets: [&[Component]; 9] = [
        &[P],
        &[P, Elevation],
        &[P, Timezone, Component::TzRules],
        &[P, TimezoneIndex, Component::TzRules],
        &[P, Names],
        &[P, Names, Search],
        &[P, Names, Search, Admin],
        &[P, AdminIndex, Component::Boundaries],
        &[P, Component::Extra],
    ];
    for list in sets {
        let set = Components::of(list);
        for verify in [Verify::Eager, Verify::Lazy] {
            let e = open(holed(&whole, set), Some(set), verify);
            let present =
                Components::of(&set.iter().filter(|&c| has.contains(c)).collect::<Vec<_>>());
            assert_eq!(e.layers().next().unwrap().components(), present, "{list:?}");
            let got = answers(&e, &probe, &points, &queries);
            let want = Answers {
                get: full.get.clone(),
                reverse: full.reverse.clone(),
                boxes: full.boxes.clone(),
                elevation: expect(&full.elevation, set, &[Elevation], Elevation),
                name: expect(&full.name, set, &[Names], Names),
                tz_of: expect(&full.tz_of, set, &[Timezone, TimezoneIndex], Timezone),
                tz_at: expect(&full.tz_at, set, &[TimezoneIndex], TimezoneIndex),
                admin_at: expect(&full.admin_at, set, &[AdminIndex], AdminIndex),
                ancestors: expect(&full.ancestors, set, &[Admin], Admin),
                search: expect(&full.search, set, &[Search], Search),
                // A context reads the hierarchy where the pack has one.
                context: if !set.contains(Search) {
                    Err(Search)
                } else if has.contains(Admin) && !set.contains(Admin) {
                    Err(Admin)
                } else {
                    full.context.clone()
                },
            };
            assert_eq!(got, want, "{list:?}, {verify:?}");
        }
    }
}

#[test]
fn a_set_without_what_it_needs_is_refused() {
    let whole = synthetic(200, 3).encode();
    let refused = |list: &[Component]| {
        let options = LayerOptions {
            installed: Some(Components::of(list)),
            ..LayerOptions::default()
        };
        match Layer::from_bytes(whole.clone(), options) {
            Err(Error::MissingComponent { component, .. }) => Some(component),
            Ok(_) => None,
            Err(e) => panic!("{e}"),
        }
    };
    assert_eq!(refused(&[Component::Point]), None);
    assert_eq!(refused(&[]), Some(Component::Point), "no point");
    assert_eq!(
        refused(&[Component::Point, Component::Search]),
        Some(Component::Names)
    );
    assert_eq!(refused(&[Component::Elevation]), Some(Component::Point));
    // Substring search ranks with search (D-199).
    let mut m = synthetic(200, 3);
    m.with_substring();
    let refused_in = |list: &[Component]| {
        let options = LayerOptions {
            installed: Some(Components::of(list)),
            ..LayerOptions::default()
        };
        match Layer::from_bytes(m.encode(), options) {
            Err(Error::MissingComponent { component, .. }) => Some(component),
            Ok(_) => None,
            Err(e) => panic!("{e}"),
        }
    };
    let (p, n, s) = (Component::Point, Component::Names, Component::Search);
    assert_eq!(refused_in(&[p, n, Component::Substring]), Some(s));
    assert_eq!(refused_in(&[p, n, s, Component::Substring]), None);
    assert_eq!(refused_in(&[p, n, s]), None);
    // A pack without rules (the baseline, a pack built before D-196) installs its zones alone.
    assert_eq!(refused(&[Component::Point, Component::Timezone]), None);
    // A pack with rules: a zone needs them (D-196).
    let mut m = synthetic(200, 3);
    let utc = tz_rules::ZoneRules {
        version: 2,
        transitions: Vec::new(),
        types: vec![tz_rules::TypeRow {
            offset: 0,
            is_dst: false,
            abbr: "UTC".into(),
        }],
        footer: tz_rules::parse_footer("UTC0").unwrap(),
        first_abbr: 0,
    };
    let zone = tz_rules::ZoneInput {
        name: "Etc/UTC",
        rules: &utc,
        place: None,
    };
    m.raw
        .push((Kind::TZ_RULES, 1, tz_rules::encode(&[zone], false).unwrap()));
    let with_rules = m.encode();
    let options = LayerOptions {
        installed: Some(Components::of(&[Component::Point, Component::Timezone])),
        ..LayerOptions::default()
    };
    match Layer::from_bytes(with_rules, options) {
        Err(Error::MissingComponent { component, .. }) => {
            assert_eq!(component, Component::TzRules);
        }
        other => panic!("zones without their rules: {:?}", other.err()),
    }
}
