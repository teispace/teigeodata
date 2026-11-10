//! The world part of the embedded baseline (`forge world`, D-147), alone and under a country's
//! slice and pack. Ignored: it needs the built files.
//!
//! ```text
//! TGD_WORLD=…/world-baseline-main.tgd [TGD_BASELINE=…/np-baseline-main.tgd TGD_PACK=…/np-places-main.tgd]
//!   cargo test --release -p teigeodata-engine --all-features --test world -- --ignored --nocapture
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    reason = "tests: failures panic with their message; the gates report their measurements"
)]

use teigeodata_core::PlaceKind;
use teigeodata_engine::{Engine, Layer, LayerOptions, Scratch};
use teigeodata_format::idmap::IdMap;
use teigeodata_format::{Kind, Pack};

fn read(var: &str) -> Option<Vec<u8>> {
    std::env::var(var).ok().map(|p| std::fs::read(p).unwrap())
}

fn baseline(priority: i32) -> LayerOptions {
    LayerOptions {
        baseline: true,
        priority,
        ..LayerOptions::default()
    }
}

/// Alone: each city hangs from one country, and `admin_at` at the city finds that country (the
/// containment the forge computed on the source polygons, measured at the stored outlines).
#[test]
#[ignore = "needs TGD_WORLD"]
fn every_city_hangs_from_the_country_around_it() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let ids = IdMap::open(&Pack::open(&world).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let mut e = Engine::new();
    e.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let (mut up, mut units) = (Vec::new(), Vec::new());
    let mut s = Scratch::default();
    let (mut cities, mut countries, mut orphans, mut agree) = (0_usize, 0_usize, 0_usize, 0_usize);
    let mut off = Vec::new();
    let mut buf = [0_u8; 512];
    for r in 0..ids.len() {
        let p = e.get(ids.id(r).unwrap()).unwrap().unwrap();
        e.ancestors(&p, &mut up).unwrap();
        match p.kind {
            Some(PlaceKind::Country) => {
                countries += 1;
                assert!(up.is_empty(), "a country has no parent");
                continue;
            }
            Some(PlaceKind::Settlement) => cities += 1,
            k => panic!("unexpected kind {k:?}"),
        }
        let Some(country) = up.first() else {
            orphans += 1;
            continue;
        };
        assert_eq!((up.len(), country.kind), (1, Some(PlaceKind::Country)));
        e.admin_at(p.point.to_e7(), &mut s, &mut units).unwrap();
        if units.iter().any(|u| u.id == country.id) {
            agree += 1;
        } else if off.len() < 8 {
            let name = e.name(&p, &mut buf).unwrap().unwrap_or_default().to_owned();
            off.push(name);
        }
    }
    let rate = agree as f64 / (cities - orphans) as f64;
    println!(
        "{countries} countries, {cities} cities ({orphans} without a country); admin_at agrees \
         with the parent for {agree} ({:.2}%); first others: {off:?}",
        rate * 100.0
    );
    assert!(countries > 200 && cities > 30_000);
    assert!(
        orphans * 1000 < cities,
        "{orphans} cities without a country"
    );
    assert!(
        rate >= 0.98,
        "admin_at agrees with the parent for {rate:.4}"
    );
}

/// Under a country's slice: the world's row for the country carries the slice's ID, so the
/// slice answers for it; a point in the country answers the slice's country, a city outside it
/// the world's; and with the pack installed the pack answers both.
#[test]
#[ignore = "needs TGD_WORLD, TGD_BASELINE and TGD_PACK"]
fn the_slice_and_the_pack_answer_for_their_country() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let slice = read("TGD_BASELINE").expect("TGD_BASELINE");
    let pack = read("TGD_PACK").expect("TGD_PACK");
    let mut base = Engine::new();
    let w = base.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let sl = base.add(Layer::from_bytes(slice.clone(), baseline(1)).unwrap());
    // The slice's country, by its kind, and the world's row with the same ID.
    let slice_ids =
        IdMap::open(&Pack::open(&slice).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let country = (0..slice_ids.len())
        .map(|r| base.get(slice_ids.id(r).unwrap()).unwrap().unwrap())
        .find(|p| p.kind == Some(PlaceKind::Country))
        .unwrap();
    assert_eq!(country.layer, sl, "the slice answers for its country");
    let mut alone = Engine::new();
    alone.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let in_world = alone.get(country.id).unwrap();
    assert!(
        in_world.is_some(),
        "the world holds the slice's country ID (ID gate)"
    );
    let (mut units, mut s) = (Vec::new(), Scratch::default());
    // Every place of the slice: `admin_at` names the slice's country there.
    for r in 0..slice_ids.len() {
        let p = base.get(slice_ids.id(r).unwrap()).unwrap().unwrap();
        base.admin_at(p.point.to_e7(), &mut s, &mut units).unwrap();
        assert!(
            units.iter().any(|u| u.id == country.id),
            "{:?}: {:?}",
            p.id,
            units.iter().map(|u| u.id).collect::<Vec<_>>()
        );
        assert!(units.iter().all(|u| u.layer == sl), "the slice's own units");
    }
    // A world city answers from the world layer, with its country.
    let world_ids =
        IdMap::open(&Pack::open(&world).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let mut up = Vec::new();
    let mut checked = 0;
    for r in (0..world_ids.len()).step_by(97) {
        let p = base.get(world_ids.id(r).unwrap()).unwrap().unwrap();
        if p.kind != Some(PlaceKind::Settlement) {
            continue;
        }
        assert_eq!(p.layer, w);
        base.ancestors(&p, &mut up).unwrap();
        assert!(up.len() <= 1);
        checked += 1;
    }
    assert!(checked > 100);
    // With the pack: it answers for the country.
    let mut full = base.clone();
    let pk = full.add(Layer::from_bytes(pack, LayerOptions::default()).unwrap());
    assert_eq!(full.get(country.id).unwrap().unwrap().layer, pk);
    println!("country {:?}; {checked} world cities checked", country.id);
}
