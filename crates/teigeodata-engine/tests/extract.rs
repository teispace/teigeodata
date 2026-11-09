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
    println!("{cities} cities checked");
}
