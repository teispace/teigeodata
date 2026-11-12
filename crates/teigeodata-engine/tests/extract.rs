//! `teigeodata_format::extract` (D-145): a pack of some rows answers for them exactly as the
//! whole pack does, each kept row's parent is its nearest kept ancestor, and admin geometry at
//! a coarser level of detail stays valid and finds each kept unit at its label point.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    reason = "tests: failures panic with their message; rows fit u32; one gate reads as one story; the ignored gates report their measurements"
)]

mod common;

use common::synthetic;
use teigeodata_core::PlaceKind;
use teigeodata_engine::{Engine, Layer, LayerOptions, Place, Scratch};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::polygons::Polygons;
use teigeodata_format::region::{Options, Region};
use teigeodata_format::{Kind, Pack, Writer, extract};

fn write(sections: &[extract::Body], address: &str) -> Vec<u8> {
    let refs: Vec<(Kind, teigeodata_format::Encoding, u64, &[u8])> = sections
        .iter()
        .map(|(k, e, n, b)| (*k, *e, *n, b.as_slice()))
        .collect();
    Writer::new(address, "places", "2026.11.0")
        .unwrap()
        .pack(&refs, None)
        .unwrap()
}

fn engine(bytes: Vec<u8>) -> Engine {
    let mut e = Engine::new();
    e.add(Layer::from_bytes(bytes, LayerOptions::default()).unwrap());
    e
}

/// What a place is, apart from the pack it is read from.
fn same(a: &Engine, x: &Place, b: &Engine, y: &Place) {
    let (mut p, mut q) = ([0_u8; 512], [0_u8; 512]);
    assert_eq!(
        (x.id, x.point, x.kind, x.admin_level, x.flags),
        (y.id, y.point, y.kind, y.admin_level, y.flags)
    );
    assert_eq!(a.name(x, &mut p).unwrap(), b.name(y, &mut q).unwrap());
    assert_eq!(a.elevation(x).unwrap(), b.elevation(y).unwrap());
}

#[test]
fn a_slice_answers_for_its_rows_as_the_whole_pack() {
    let model = synthetic(3_000, 21);
    let whole_bytes = model.encode();
    let pack = Pack::open(&whole_bytes).unwrap();
    // The country, every other district and every 9th settlement; no zones. A settlement whose
    // district is dropped takes the country as its parent.
    let keep: Vec<u32> = (0..model.ids.len() as u32)
        .filter(|&r| match model.kinds[r as usize] {
            PlaceKind::Country => true,
            PlaceKind::Admin => r % 2 == 0,
            PlaceKind::Settlement => r % 9 == 0,
            _ => false,
        })
        .collect();
    let slice = write(
        &extract::rows(&pack, &keep, 0, &Options::default()).unwrap(),
        "np-slice",
    );
    let (whole, part) = (engine(whole_bytes.clone()), engine(slice));
    let kept: std::collections::BTreeSet<u32> = keep.iter().copied().collect();
    let mut s = Scratch::default();
    let (mut hits, mut up_w, mut up_p) = (Vec::new(), Vec::new(), Vec::new());
    let mut buf = [0_u8; 512];
    for r in 0..model.ids.len() as u32 {
        let id = model.ids[r as usize];
        let Some(p) = part.get(id).unwrap() else {
            assert!(!kept.contains(&r), "row {r} kept but missing");
            continue;
        };
        assert!(kept.contains(&r), "row {r} dropped but present");
        let w = whole.get(id).unwrap().unwrap();
        same(&whole, &w, &part, &p);
        // The chain is the whole pack's, cut to the kept rows.
        whole.ancestors(&w, &mut up_w).unwrap();
        part.ancestors(&p, &mut up_p).unwrap();
        let want: Vec<_> = up_w
            .iter()
            .filter(|a| kept.contains(&a.row))
            .map(|a| a.id)
            .collect();
        assert_eq!(up_p.iter().map(|a| a.id).collect::<Vec<_>>(), want);
        // Its own name finds it.
        let name = part.name(&p, &mut buf).unwrap().unwrap().to_owned();
        part.search(&name, 50, &mut s, &mut hits).unwrap();
        assert!(hits.iter().any(|h| h.place.id == id), "{name}");
    }
    assert!(keep.len() > 300);
    // Some kept districts and some dropped ones.
    let districts = |m: u32| {
        (0..model.ids.len() as u32)
            .filter(|&r| model.kinds[r as usize] == PlaceKind::Admin && r % 2 == m)
            .count()
    };
    assert!(districts(0) > 0 && districts(1) > 0);
}

/// `PLACE_TZ` (D-151): a slice that keeps its places' timezone areas gives every kept place the
/// whole pack's zone; one that drops them has no zones, rather than wrong ones.
#[test]
fn a_slice_keeps_the_zones_of_its_places() {
    let model = synthetic(3_000, 23);
    let whole_bytes = model.encode();
    let pack = Pack::open(&whole_bytes).unwrap();
    let whole = engine(whole_bytes.clone());
    let rows_for = |zones: bool| -> Vec<u32> {
        (0..model.ids.len() as u32)
            .filter(|&r| match model.kinds[r as usize] {
                PlaceKind::TimezoneArea => zones,
                PlaceKind::Country | PlaceKind::Admin => true,
                PlaceKind::Settlement => r % 5 == 0,
                _ => false,
            })
            .collect()
    };
    for zones in [true, false] {
        let keep = rows_for(zones);
        let sections = extract::rows(&pack, &keep, 0, &Options::default()).unwrap();
        assert_eq!(
            sections.iter().any(|s| s.0 == Kind::PLACE_TZ),
            zones,
            "PLACE_TZ only with the zone rows"
        );
        let part = engine(write(&sections, "np-slice"));
        let mut with_zone = 0;
        for &r in &keep {
            let id = model.ids[r as usize];
            let (w, p) = (
                whole.get(id).unwrap().unwrap(),
                part.get(id).unwrap().unwrap(),
            );
            if p.kind == Some(PlaceKind::TimezoneArea) {
                continue;
            }
            let got = part.tz_of(&p).unwrap().map(|z| z.id);
            if zones {
                assert_eq!(got, whole.tz_of(&w).unwrap().map(|z| z.id), "row {r}");
                with_zone += usize::from(got.is_some());
            } else {
                assert_eq!(got, None, "row {r}: no zone rows, no zone");
            }
        }
        assert!(!zones || with_zone > 100, "{with_zone}");
    }
}

/// A merge (D-153): two parts that share rows answer as one layer exactly as they do as two
/// layers, the later part above: the same place, name and chain for every ID, the same hits for
/// every name; a shared row is the later part's, once.
#[test]
fn a_merge_answers_as_its_parts_as_layers() {
    let model = synthetic(3_000, 31);
    let whole_bytes = model.encode();
    let pack = Pack::open(&whole_bytes).unwrap();
    let rows_every = |m: u32| -> Vec<u32> {
        (0..model.ids.len() as u32)
            .filter(|&r| match model.kinds[r as usize] {
                PlaceKind::Country | PlaceKind::Admin => true,
                PlaceKind::Settlement => r % m == 0,
                _ => false,
            })
            .collect()
    };
    let (a, b) = (rows_every(2), rows_every(3));
    let part = |keep| extract::Part {
        pack: &pack,
        keep,
        lod: 0,
        names: None,
        arcs: None,
    };
    let merged = write(
        &extract::merge(&[part(&a), part(&b)], &Options::default()).unwrap(),
        "np",
    );
    let one = engine(merged);
    let mut two = Engine::new();
    let layer = |keep: &[u32], priority| {
        let bytes = write(
            &extract::rows(&pack, keep, 0, &Options::default()).unwrap(),
            "np",
        );
        Layer::from_bytes(
            bytes,
            LayerOptions {
                priority,
                ..LayerOptions::default()
            },
        )
        .unwrap()
    };
    two.add(layer(&a, 0));
    two.add(layer(&b, 1));
    let both: std::collections::BTreeSet<u32> = a.iter().chain(&b).copied().collect();
    let (mut s, mut up_a, mut up_b, mut h_a, mut h_b) = (
        Scratch::default(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let mut buf = [0_u8; 512];
    for &r in &both {
        let id = model.ids[r as usize];
        let (x, y) = (one.get(id).unwrap().unwrap(), two.get(id).unwrap().unwrap());
        same(&one, &x, &two, &y);
        one.ancestors(&x, &mut up_a).unwrap();
        two.ancestors(&y, &mut up_b).unwrap();
        let ids = |v: &[Place]| v.iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&up_a), ids(&up_b), "row {r}");
        if r % 7 == 0 {
            let name = one.name(&x, &mut buf).unwrap().unwrap().to_owned();
            one.search(&name, 10, &mut s, &mut h_a).unwrap();
            two.search(&name, 10, &mut s, &mut h_b).unwrap();
            let hits = |h: &[teigeodata_engine::Hit]| {
                h.iter().map(|x| (x.place.id, x.score)).collect::<Vec<_>>()
            };
            assert_eq!(hits(&h_a), hits(&h_b), "{name}");
        }
    }
    let rows = Pack::open(&write(
        &extract::merge(&[part(&a), part(&b)], &Options::default()).unwrap(),
        "np",
    ))
    .unwrap()
    .section(Kind::PLACES)
    .unwrap()
    .item_count();
    assert_eq!(rows, both.len() as u64, "a shared row once");
    assert!(a.len() + b.len() > both.len() + 100, "the parts share rows");
}

/// A name rule (D-152): the slice keeps the canonical and the names the rule keeps, and its
/// SEARCH is rebuilt from them, so a kept alias finds its place and a dropped one does not.
#[test]
fn a_name_rule_keeps_names_and_their_keys_only() {
    use teigeodata_format::names::{NameRecord, Tag as Lang};
    let mut model = synthetic(2_000, 29);
    let (fr, de) = (Lang::new("fr").unwrap(), Lang::new("de").unwrap());
    // Each settlement gains a French and a German alias, keyed like its other names.
    for r in 0..model.ids.len() {
        if model.kinds[r] != PlaceKind::Settlement {
            continue;
        }
        for (lang, text) in [(fr, format!("Ville {r}")), (de, format!("Stadt {r}"))] {
            let f = teigeodata_names::fold(&text);
            let mut k = vec![teigeodata_format::search::EXACT];
            k.extend_from_slice(f.as_bytes());
            model.keys.entry(k).or_default().insert(r as u32);
            let mut name = model.names[r][0].clone();
            (name.text, name.lang) = (text, Some(lang));
            model.names[r].push(name);
        }
    }
    let whole_bytes = model.encode();
    let pack = Pack::open(&whole_bytes).unwrap();
    let keep: Vec<u32> = (0..model.ids.len() as u32)
        .filter(|&r| model.kinds[r as usize] != PlaceKind::TimezoneArea && r % 3 == 0)
        .collect();
    let keep_name = |_: u32, _: usize, n: &NameRecord<'_>| n.lang != Some(de);
    let keys = |text: &str,
                _: teigeodata_format::names::Origin,
                row: u32,
                maps: &mut teigeodata_format::search::Maps| {
        let f = teigeodata_names::fold(text);
        maps.loose
            .entry(f.replace(' ', ""))
            .or_default()
            .insert(row);
        maps.exact.entry(f).or_default().insert(row);
    };
    let rule = extract::NameRule {
        keep: &keep_name,
        keys: &keys,
    };
    let sections = extract::rows_with(&pack, &keep, 0, &Options::default(), Some(&rule)).unwrap();
    let part = engine(write(&sections, "np-slice"));
    let (mut s, mut hits, mut buf) = (Scratch::default(), Vec::new(), [0_u8; 512]);
    let mut finds = |q: &str, id| {
        part.search(q, 50, &mut s, &mut hits).unwrap();
        hits.iter().any(|h| h.place.id == id)
    };
    let mut aliases = 0;
    for &r in &keep {
        let id = model.ids[r as usize];
        let p = part.get(id).unwrap().unwrap();
        let canonical = part.name(&p, &mut buf).unwrap().unwrap().to_owned();
        assert_eq!(canonical, model.names[r as usize][0].text, "row {r}");
        assert!(finds(&canonical, id), "{canonical}");
        if model.kinds[r as usize] == PlaceKind::Settlement {
            assert!(
                finds(&format!("Ville {r}"), id),
                "the kept alias of row {r}"
            );
            assert!(
                !finds(&format!("Stadt {r}"), id),
                "the dropped alias of row {r}"
            );
            aliases += 1;
        }
    }
    assert!(aliases > 100, "{aliases}");
    // A rule that keeps nothing still keeps each canonical name.
    let none = |_: u32, _: usize, _: &NameRecord<'_>| false;
    let rule = extract::NameRule {
        keep: &none,
        keys: &keys,
    };
    let part = engine(write(
        &extract::rows_with(&pack, &keep, 0, &Options::default(), Some(&rule)).unwrap(),
        "np-slice",
    ));
    for &r in &keep {
        let p = part.get(model.ids[r as usize]).unwrap().unwrap();
        assert_eq!(
            part.name(&p, &mut buf).unwrap(),
            Some(model.names[r as usize][0].text.as_str())
        );
    }
}

/// The real pack (`TGD_PACK`): its country and level-1 units at a 500 m level, plus a sample of
/// places.
#[test]
#[ignore = "needs a real pack: TGD_PACK=path/to/pack.tgd"]
fn a_real_slice_keeps_valid_geometry() {
    let path = std::env::var("TGD_PACK").expect("TGD_PACK");
    let bytes = std::fs::read(path).unwrap();
    let pack = Pack::open(&bytes).unwrap();
    let whole = engine(bytes.clone());
    let places =
        teigeodata_format::places::Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
    let keep: Vec<u32> = (0..places.len() as u32)
        .filter(|&r| {
            let level = places.admin_level(r as usize).unwrap();
            match places.kind(r as usize) {
                Some(PlaceKind::Country) => true,
                Some(PlaceKind::Admin) => level <= 1,
                _ => r % 50 == 0,
            }
        })
        .collect();
    let arcs = Arcs::open(&pack.section(Kind::ARCS).unwrap()).unwrap();
    let lod = extract::lod_within(&arcs, 50_000);
    assert!(lod > 0, "the pack's arcs have a level within 500 m");
    // A/B of the slice's region index: bytes and `admin_at` time.
    for (leaf_max, fine_cells) in [(32, 1 << 18), (32, 0), (32, 1 << 12), (32, 1 << 14)] {
        let o = Options {
            leaf_max,
            fine_cells,
            ..Options::default()
        };
        let s = extract::rows(&pack, &keep, lod, &o).unwrap();
        let e = engine(write(&s, "np-ab"));
        let mut sc = Scratch::default();
        let mut out = Vec::new();
        let pts: Vec<_> = (0..places.len())
            .step_by(97)
            .map(|r| places.point(r).unwrap().to_e7())
            .collect();
        for &p in &pts {
            e.admin_at(p, &mut sc, &mut out).unwrap();
        }
        let t = std::time::Instant::now();
        for _ in 0..5 {
            for &p in &pts {
                e.admin_at(p, &mut sc, &mut out).unwrap();
            }
        }
        println!(
            "region leaf_max {leaf_max} fine {fine_cells}: ADMIN_REGION {} B, admin_at {:.0} ns",
            s.iter()
                .find(|x| x.0 == Kind::ADMIN_REGION)
                .unwrap()
                .3
                .len(),
            t.elapsed().as_nanos() as f64 / (5 * pts.len()) as f64
        );
    }
    let sections = extract::rows(&pack, &keep, lod, &Options::default()).unwrap();
    let slice = write(&sections, "np-slice");
    let p = Pack::open(&slice).unwrap();
    // The format's own checks of the new geometry.
    let a = Arcs::open(&p.section(Kind::ARCS).unwrap()).unwrap();
    a.check().unwrap();
    let polys = Polygons::open(&p.section(Kind::POLYGONS).unwrap()).unwrap();
    polys.check(&a, keep.len()).unwrap();
    Region::open_on_arcs(&p.section(Kind::ADMIN_REGION).unwrap(), &a)
        .unwrap()
        .check()
        .unwrap();
    let part = engine(slice.clone());
    let mut s = Scratch::default();
    let mut units = Vec::new();
    let n = p.section(Kind::POLYGONS).unwrap().item_count() as usize;
    assert!(n >= 2, "the country and its units: {n}");
    for i in 0..n {
        let label = polys.label(i).unwrap();
        let row = polys.row(i).unwrap();
        part.admin_at(label, &mut s, &mut units).unwrap();
        assert!(
            units.iter().any(|u| u.row == row),
            "unit {i} not found at its label"
        );
    }
    for (i, &r) in keep.iter().enumerate() {
        let id = places.point(r as usize).map(|_| ());
        assert!(id.is_some());
        let w = whole
            .get(
                teigeodata_format::idmap::IdMap::open(&pack.section(Kind::IDMAP).unwrap())
                    .unwrap()
                    .id(r as usize)
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
        let q = part.get(w.id).unwrap().unwrap();
        assert_eq!(q.row as usize, i);
        same(&whole, &w, &part, &q);
    }
    println!(
        "slice: {} of {} rows, lod {lod} ({} cm), {} B of {} B; ARCS {} B",
        keep.len(),
        places.len(),
        arcs.tolerance_cm(lod).unwrap_or(0),
        slice.len(),
        bytes.len(),
        sections
            .iter()
            .find(|s| s.0 == Kind::ARCS)
            .map_or(0, |s| s.3.len())
    );
}

/// The forge's baseline slice (`TGD_BASELINE`) under its pack (`TGD_PACK`): the pack answers
/// every ID the slice holds, with its full chain; the slice alone answers each city with its
/// level-1 unit and country, and `admin_at` at the city finds the same two (D-145).
#[test]
#[ignore = "needs TGD_PACK and TGD_BASELINE"]
fn the_baseline_slice_under_its_pack() {
    let pack = std::fs::read(std::env::var("TGD_PACK").expect("TGD_PACK")).unwrap();
    let base = std::fs::read(std::env::var("TGD_BASELINE").expect("TGD_BASELINE")).unwrap();
    let baseline = LayerOptions {
        baseline: true,
        ..LayerOptions::default()
    };
    let mut both = Engine::new();
    let b = both.add(Layer::from_bytes(base.clone(), baseline).unwrap());
    let p = both.add(Layer::from_bytes(pack, LayerOptions::default()).unwrap());
    let mut alone = Engine::new();
    alone.add(Layer::from_bytes(base.clone(), baseline).unwrap());
    let ids = teigeodata_format::idmap::IdMap::open(
        &Pack::open(&base).unwrap().section(Kind::IDMAP).unwrap(),
    )
    .unwrap();
    let (mut up, mut units) = (Vec::new(), Vec::new());
    let mut s = Scratch::default();
    let mut cities = 0;
    for r in 0..ids.len() {
        let id = ids.id(r).unwrap();
        let w = both.get(id).unwrap().unwrap();
        assert_eq!(w.layer, p, "the pack wins {id:?}");
        assert_ne!(w.layer, b);
        let x = alone.get(id).unwrap().unwrap();
        if x.kind != Some(PlaceKind::Settlement) {
            continue;
        }
        cities += 1;
        alone.ancestors(&x, &mut up).unwrap();
        let chain: Vec<_> = up.iter().map(|a| (a.kind, a.admin_level)).collect();
        assert_eq!(
            chain,
            [(Some(PlaceKind::Admin), 1), (Some(PlaceKind::Country), 0)],
            "{id:?}"
        );
        both.ancestors(&w, &mut up).unwrap();
        assert!(up.len() > 2, "the pack's chain is deeper: {id:?}");
        alone.admin_at(x.point.to_e7(), &mut s, &mut units).unwrap();
        let want: Vec<_> = {
            alone.ancestors(&x, &mut up).unwrap();
            let mut v: Vec<_> = up.iter().map(|a| a.id).collect();
            v.sort_unstable();
            v
        };
        let mut got: Vec<_> = units.iter().map(|u| u.id).collect();
        got.sort_unstable();
        assert_eq!(got, want, "admin_at at {id:?}");
    }
    assert!(cities > 10);
    // Search keys come from the names the slice keeps (D-152): each kept name finds its row.
    let names = teigeodata_format::names::Names::open(
        &Pack::open(&base).unwrap().section(Kind::NAMES).unwrap(),
    )
    .unwrap();
    let (mut buf, mut hits, mut checked) = (vec![0_u8; 1 << 16], Vec::new(), 0);
    for r in 0..ids.len() {
        let id = ids.id(r).unwrap();
        for n in names.names(r) {
            let text = n.text(&mut buf).unwrap().to_owned();
            alone.search(&text, 50, &mut s, &mut hits).unwrap();
            assert!(
                hits.iter().any(|h| h.place.id == id),
                "{text:?} does not find {id:?}"
            );
            checked += 1;
        }
    }
    println!("{cities} cities checked; {checked} names find their row");
}

/// Each section's verdict from `check::sections`, by kind.
fn verdicts(bytes: &[u8]) -> Vec<(Kind, Result<bool, teigeodata_format::Error>)> {
    let pack = Pack::open_verified(bytes).unwrap();
    let mut v = Vec::new();
    teigeodata_format::check::sections(&pack, |k, r| v.push((k, r)));
    v
}

#[test]
fn every_section_checks_whole_and_a_mismatch_is_named() {
    let model = synthetic(2_000, 5);
    let whole = model.encode();
    let v = verdicts(&whole);
    assert!(v.len() > 10, "{v:?}");
    for (k, r) in &v {
        assert!(matches!(r, Ok(true)), "{k}: {r:?}");
    }
    // ELEVATION one row short: written and hashed correctly, refused by the deep check alone.
    let pack = Pack::open(&whole).unwrap();
    let all: Vec<u32> = (0..model.ids.len() as u32).collect();
    let mut sections = extract::rows(&pack, &all, 0, &Options::default()).unwrap();
    let short = vec![Some(7_i16); model.ids.len() - 1];
    let e = sections
        .iter_mut()
        .find(|s| s.0 == Kind::ELEVATION)
        .unwrap();
    e.3 = teigeodata_format::elevation::encode(&short)
        .unwrap()
        .unwrap();
    e.2 = short.len() as u64;
    let v = verdicts(&write(&sections, "np"));
    for (k, r) in &v {
        if *k == Kind::ELEVATION {
            assert!(r.is_err(), "the short ELEVATION passed");
        } else {
            assert!(matches!(r, Ok(true)), "{k}: {r:?}");
        }
    }
}

/// `admin_at`'s rows at each point.
type Answers = Vec<Vec<u32>>;

/// TG-16: what each level of detail of a real pack's outlines (`TGD_PACK`) costs and changes.
/// For every level: the geometry sections' bytes, the vertices kept, and the share of
/// `admin_at` answers that differ from full resolution, at every place's point (where people
/// ask) and at points within 100 m of a border vertex (the worst case).
#[test]
#[ignore = "needs a real pack: TGD_PACK=path/to/pack.tgd"]
fn outline_levels_measured() {
    let path = std::env::var("TGD_PACK").expect("TGD_PACK");
    let bytes = std::fs::read(path).unwrap();
    let pack = Pack::open(&bytes).unwrap();
    let places =
        teigeodata_format::places::Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
    let arcs = Arcs::open(&pack.section(Kind::ARCS).unwrap()).unwrap();
    let keep: Vec<u32> = (0..places.len() as u32).collect();
    let n_arcs = pack.section(Kind::ARCS).unwrap().item_count() as usize;
    let at_places: Vec<_> = (0..places.len())
        .map(|r| places.point(r).unwrap().to_e7())
        .collect();
    // Every 7th full-resolution vertex, moved up to 100 m (about 900 units of 1e-7°) by a
    // fixed pseudo-random step.
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut step = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        ((seed >> 33) % 1_801) as i32 - 900
    };
    let mut near = Vec::new();
    for a in 0..n_arcs {
        for v in arcs.points(a, false, 0).step_by(7) {
            let (dy, dx) = (step(), step());
            near.extend(teigeodata_core::Point::<teigeodata_core::E7>::from_units(
                v.lat() + dy,
                v.lon() + dx,
            ));
        }
    }
    let answers = |e: &Engine, pts: &[teigeodata_core::Point<teigeodata_core::E7>]| {
        let mut s = Scratch::default();
        let mut out = Vec::new();
        pts.iter()
            .map(|&p| {
                e.admin_at(p, &mut s, &mut out).unwrap();
                let mut rows: Vec<u32> = out.iter().map(|u| u.row).collect();
                rows.sort_unstable();
                rows
            })
            .collect::<Vec<_>>()
    };
    let geometry = [
        Kind::ARCS,
        Kind::POLYGONS,
        Kind::POLY_INDEX,
        Kind::ADMIN_REGION,
    ];
    let mut full: Option<(Answers, Answers)> = None;
    println!(
        "{} places, {} arcs, {} near-border points",
        places.len(),
        n_arcs,
        near.len()
    );
    println!(
        "| level | tolerance | vertices | geometry B | places changed | near-border changed |"
    );
    for lod in 0..=6_u8 {
        let Some(tol) = (lod == 0).then_some(0).or_else(|| arcs.tolerance_cm(lod)) else {
            continue;
        };
        let s = extract::rows(&pack, &keep, lod, &Options::default()).unwrap();
        let geo: usize = s
            .iter()
            .filter(|x| geometry.contains(&x.0))
            .map(|x| x.3.len())
            .sum();
        let vertices: usize = (0..n_arcs).map(|a| arcs.count(a, lod)).sum();
        let e = engine(write(&s, "np-lod"));
        let (p, n) = (answers(&e, &at_places), answers(&e, &near));
        let (fp, fn_) = full.get_or_insert_with(|| (p.clone(), n.clone()));
        let diff = |a: &[Vec<u32>], b: &[Vec<u32>]| {
            a.iter().zip(b).filter(|(x, y)| x != y).count() as f64 * 100.0 / a.len() as f64
        };
        println!(
            "| {lod} | {} m | {vertices} | {geo} | {:.3}% | {:.2}% |",
            tol / 100,
            diff(&p, fp),
            diff(&n, fn_)
        );
    }
}

/// TG-16: the admin index of a real pack or layer (`TGD_PACK`) rebuilt with each leaf size:
/// its bytes, and `admin_at`'s time at every 13th place, answers checked equal to the first.
#[test]
#[ignore = "needs a real pack: TGD_PACK=path/to/pack.tgd"]
fn region_leaf_sizes_measured() {
    let path = std::env::var("TGD_PACK").expect("TGD_PACK");
    let bytes = std::fs::read(path).unwrap();
    let pack = Pack::open(&bytes).unwrap();
    let places =
        teigeodata_format::places::Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
    let keep: Vec<u32> = (0..places.len() as u32).collect();
    let pts: Vec<_> = (0..places.len())
        .step_by(13)
        .map(|r| places.point(r).unwrap().to_e7())
        .collect();
    let mut first: Option<Vec<Vec<u32>>> = None;
    println!("| leaf_max | fine cells | ADMIN_REGION B | admin_at ns |");
    for leaf_max in [32, 40, 48, 64, 96] {
        for fine_cells in [1 << 12] {
            let o = Options {
                leaf_max,
                fine_cells,
                ..Options::default()
            };
            let s = extract::rows(&pack, &keep, 0, &o).unwrap();
            let e = engine(write(&s, "ab"));
            let (mut sc, mut out) = (Scratch::default(), Vec::new());
            let got: Vec<Vec<u32>> = pts
                .iter()
                .map(|&p| {
                    e.admin_at(p, &mut sc, &mut out).unwrap();
                    out.iter().map(|u| u.row).collect()
                })
                .collect();
            assert_eq!(&got, first.get_or_insert_with(|| got.clone()));
            let t = std::time::Instant::now();
            for _ in 0..5 {
                for &p in &pts {
                    e.admin_at(p, &mut sc, &mut out).unwrap();
                }
            }
            println!(
                "| {leaf_max} | {fine_cells} | {} | {:.0} |",
                s.iter()
                    .find(|x| x.0 == Kind::ADMIN_REGION)
                    .unwrap()
                    .3
                    .len(),
                t.elapsed().as_nanos() as f64 / (5 * pts.len()) as f64
            );
        }
    }
}

/// TG-16: two builds of one pack (`TGD_PACK` and `TGD_PACK_B`, the same rows) answer `admin_at`
/// alike: the share of different answers at every place of the first and at points within
/// 100 m of its border vertices, and the geometry sections' bytes of each.
#[test]
#[ignore = "needs TGD_PACK and TGD_PACK_B"]
fn two_builds_answer_admin_at_alike() {
    let read = |v: &str| std::fs::read(std::env::var(v).expect(v)).unwrap();
    let (a_bytes, b_bytes) = (read("TGD_PACK"), read("TGD_PACK_B"));
    let pack = Pack::open(&a_bytes).unwrap();
    let places =
        teigeodata_format::places::Places::open(&pack.section(Kind::PLACES).unwrap()).unwrap();
    let arcs = Arcs::open(&pack.section(Kind::ARCS).unwrap()).unwrap();
    let n_arcs = pack.section(Kind::ARCS).unwrap().item_count() as usize;
    let at_places: Vec<_> = (0..places.len())
        .map(|r| places.point(r).unwrap().to_e7())
        .collect();
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut step = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        ((seed >> 33) % 1_801) as i32 - 900
    };
    let mut near = Vec::new();
    for a in 0..n_arcs {
        for v in arcs.points(a, false, 0).step_by(7) {
            let (dy, dx) = (step(), step());
            near.extend(teigeodata_core::Point::<teigeodata_core::E7>::from_units(
                v.lat() + dy,
                v.lon() + dx,
            ));
        }
    }
    let geometry = [
        Kind::ARCS,
        Kind::POLYGONS,
        Kind::POLY_INDEX,
        Kind::ADMIN_REGION,
    ];
    let bytes_of = |b: &[u8]| {
        let p = Pack::open(b).unwrap();
        geometry
            .iter()
            .map(|&k| p.section(k).map_or(0, |s| s.decoded_length()))
            .sum::<u64>()
    };
    let (ga, gb) = (bytes_of(&a_bytes), bytes_of(&b_bytes));
    let (ea, eb) = (engine(a_bytes), engine(b_bytes));
    let answers = |e: &Engine, pts: &[teigeodata_core::Point<teigeodata_core::E7>]| {
        let (mut s, mut out) = (Scratch::default(), Vec::new());
        pts.iter()
            .map(|&p| {
                e.admin_at(p, &mut s, &mut out).unwrap();
                let mut rows: Vec<u32> = out.iter().map(|u| u.row).collect();
                rows.sort_unstable();
                rows
            })
            .collect::<Vec<_>>()
    };
    let diff = |pts: &[teigeodata_core::Point<teigeodata_core::E7>]| {
        let (x, y) = (answers(&ea, pts), answers(&eb, pts));
        x.iter().zip(&y).filter(|(p, q)| p != q).count()
    };
    let (dp, dn) = (diff(&at_places), diff(&near));
    println!(
        "geometry {ga} B → {gb} B; places changed {dp} of {} ({:.4}%); near-border changed {dn} of {} ({:.3}%)",
        at_places.len(),
        dp as f64 * 100.0 / at_places.len() as f64,
        near.len(),
        dn as f64 * 100.0 / near.len() as f64
    );
}
