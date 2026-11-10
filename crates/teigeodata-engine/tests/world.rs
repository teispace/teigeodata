//! The world part of the embedded baseline (`forge world`, D-147), alone and under a country's
//! slice and pack. Ignored: it needs the built files.
//!
//! ```text
//! TGD_WORLD=…/world-baseline-main.tgd TGD_PROBES=…/world-probes-main.tsv
//! [TGD_BASELINE=…/np-baseline-main.tgd TGD_PACK=…/np-places-main.tgd]
//!   cargo test --release -p teigeodata-engine --all-features --test world -- --ignored --nocapture
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    reason = "tests: failures panic with their message; the gates report their measurements"
)]

use std::time::Instant;

use teigeodata_core::{E7, PlaceId, PlaceKind, Point};
use teigeodata_engine::{Engine, LOCATOR_RADIUS_M, Layer, LayerOptions, Scratch};
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

/// Alone: each first-level unit hangs from its country, each city from the unit around it or,
/// off every unit, from its country (D-150), and `admin_at` at the city finds them (the
/// containment the forge computed on the source polygons, measured at the stored outlines).
/// Every `admin_at` answer is one chain: each unit it gives has the country it gives as parent.
#[test]
#[ignore = "needs TGD_WORLD"]
fn every_city_hangs_from_the_unit_and_country_around_it() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let ids = IdMap::open(&Pack::open(&world).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let mut e = Engine::new();
    e.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let (mut up, mut found, mut unit_up) = (Vec::new(), Vec::new(), Vec::new());
    let mut s = Scratch::default();
    let (mut countries, mut units, mut cities, mut under_unit) = (0_usize, 0_usize, 0_usize, 0);
    let (mut orphans, mut agree, mut unit_agree) = (0_usize, 0_usize, 0_usize);
    let mut off = Vec::new();
    let mut buf = [0_u8; 512];
    for r in 0..ids.len() {
        let p = e.get(ids.id(r).unwrap()).unwrap().unwrap();
        e.ancestors(&p, &mut up).unwrap();
        let kinds: Vec<_> = up.iter().map(|a| a.kind).collect();
        match p.kind {
            Some(PlaceKind::Country) => {
                countries += 1;
                assert!(up.is_empty(), "a country has no parent");
                continue;
            }
            Some(PlaceKind::Admin) => {
                units += 1;
                assert_eq!(
                    kinds,
                    [Some(PlaceKind::Country)],
                    "{:?}: a unit's parent",
                    p.id
                );
                continue;
            }
            Some(PlaceKind::Settlement) => cities += 1,
            k => panic!("unexpected kind {k:?}"),
        }
        let Some(country) = up.last() else {
            orphans += 1;
            continue;
        };
        let unit = match kinds.as_slice() {
            [Some(PlaceKind::Country)] => None,
            [Some(PlaceKind::Admin), Some(PlaceKind::Country)] => up.first().map(|u| u.id),
            k => panic!("{:?}: ancestors {k:?}", p.id),
        };
        under_unit += usize::from(unit.is_some());
        e.admin_at(p.point.to_e7(), &mut s, &mut found).unwrap();
        // One chain: every unit found hangs from the country found.
        let here: Vec<PlaceId> = found
            .iter()
            .filter(|u| u.kind == Some(PlaceKind::Country))
            .map(|u| u.id)
            .collect();
        assert!(here.len() <= 1, "{:?}: countries {here:?}", p.id);
        for u in found.iter().filter(|u| u.kind == Some(PlaceKind::Admin)) {
            e.ancestors(u, &mut unit_up).unwrap();
            assert_eq!(
                unit_up.last().map(|c| c.id),
                here.first().copied(),
                "{:?}: admin_at gives unit {:?} and another country",
                p.id,
                u.id
            );
        }
        if here.first() == Some(&country.id) {
            agree += 1;
        } else if off.len() < 8 {
            let name = e.name(&p, &mut buf).unwrap().unwrap_or_default().to_owned();
            off.push(name);
        }
        if unit.is_some_and(|id| found.iter().any(|u| u.id == id)) {
            unit_agree += 1;
        }
    }
    let rate = agree as f64 / (cities - orphans) as f64;
    let unit_rate = unit_agree as f64 / under_unit.max(1) as f64;
    println!(
        "{countries} countries, {units} units, {cities} cities ({orphans} without a country, \
         {under_unit} under a unit); admin_at agrees with the country for {agree} ({:.2}%), with \
         the unit for {unit_agree} ({:.2}%); first others: {off:?}",
        rate * 100.0,
        unit_rate * 100.0
    );
    assert!(countries > 200 && units > 4_000 && cities > 30_000);
    assert!(
        orphans * 1000 < cities,
        "{orphans} cities without a country"
    );
    assert!(
        under_unit * 100 >= cities * 95,
        "{under_unit} cities under a unit"
    );
    assert!(
        rate >= 0.98,
        "admin_at agrees with the parent for {rate:.4}"
    );
    assert!(
        unit_rate >= 0.95,
        "admin_at agrees with the unit for {unit_rate:.4}"
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
        assert!(up.len() <= 2, "a city's unit and country at most");
        checked += 1;
    }
    assert!(checked > 100);
    // With the pack: it answers for the country.
    let mut full = base.clone();
    let pk = full.add(Layer::from_bytes(pack, LayerOptions::default()).unwrap());
    assert_eq!(full.get(country.id).unwrap().unwrap().layer, pk);
    println!("country {:?}; {checked} world cities checked", country.id);
}

/// The locator gate (D-149): each border probe (a full-resolution source vertex, written by
/// `forge world`) asked of the stored, simplified outlines at [`LOCATOR_RADIUS_M`] finds its own
/// country; smaller radii are measured to show the margin. Also measures how many countries a
/// city far from a border gets (one is ideal) and the query time.
#[test]
#[ignore = "needs TGD_WORLD and TGD_PROBES"]
fn the_locator_finds_the_country_of_every_border_probe() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let probes = std::fs::read_to_string(std::env::var("TGD_PROBES").expect("TGD_PROBES")).unwrap();
    let probes: Vec<(Point<E7>, PlaceId)> = probes
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let mut f = l.split('\t');
            let mut n = || f.next().unwrap();
            let (lat, lon) = (n().parse().unwrap(), n().parse().unwrap());
            let id = PlaceId::from_raw(n().parse().unwrap()).unwrap();
            (Point::from_units(lat, lon).unwrap(), id)
        })
        .collect();
    assert!(probes.len() > 50_000, "{} probes", probes.len());
    let mut e = Engine::new();
    e.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let mut near = Vec::new();
    println!("| radius m | probes missed | countries per probe | µs per query |");
    for radius in [0, 250, 500, LOCATOR_RADIUS_M, 2_000] {
        let (mut missed, mut found) = (Vec::new(), 0_usize);
        let t = Instant::now();
        for &(at, id) in &probes {
            e.countries_near(at, radius, &mut near).unwrap();
            found += near.len();
            if !near.iter().any(|c| c.id == id) {
                missed.push((at.lat(), at.lon(), id));
            }
        }
        let us = t.elapsed().as_secs_f64() * 1e6 / probes.len() as f64;
        println!(
            "| {radius} | {} ({:.3}%) | {:.3} | {us:.1} |",
            missed.len(),
            missed.len() as f64 * 100.0 / probes.len() as f64,
            found as f64 / probes.len() as f64
        );
        if radius >= LOCATOR_RADIUS_M {
            assert!(
                missed.is_empty(),
                "at {radius} m: first misses {:?}",
                &missed[..missed.len().min(5)]
            );
        }
    }
    // Cities: how many countries each gets; every one gets at least the one holding it.
    let ids = IdMap::open(&Pack::open(&world).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let (mut cities, mut one, mut none) = (0_usize, 0_usize, 0_usize);
    let mut up = Vec::new();
    for r in 0..ids.len() {
        let p = e.get(ids.id(r).unwrap()).unwrap().unwrap();
        if p.kind != Some(PlaceKind::Settlement) {
            continue;
        }
        e.ancestors(&p, &mut up).unwrap();
        if up.is_empty() {
            continue;
        }
        cities += 1;
        e.countries_near(p.point.to_e7(), LOCATOR_RADIUS_M, &mut near)
            .unwrap();
        match near.len() {
            0 => none += 1,
            1 => one += 1,
            _ => {}
        }
    }
    println!(
        "{cities} cities with a country: {one} ({:.2}%) get one country at {LOCATOR_RADIUS_M} m, \
         {none} none",
        one as f64 * 100.0 / cities as f64
    );
    // A city in no country at 1 km is a coastal point more than 1 km off the 1:10m coastline
    // (the cities whose parent fell back to their code's feature, D-147): at most 0.5%.
    assert!(none * 200 < cities, "{none} cities in no country");
}

/// `missing_packs`: with only the baseline, a point in the slice's country names that country
/// (its pack is not installed); with its pack installed, nothing.
#[test]
#[ignore = "needs TGD_WORLD, TGD_BASELINE and TGD_PACK"]
fn missing_packs_names_the_pack_to_fetch() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let slice = read("TGD_BASELINE").expect("TGD_BASELINE");
    let pack = read("TGD_PACK").expect("TGD_PACK");
    let mut e = Engine::new();
    e.add(Layer::from_bytes(world, baseline(0)).unwrap());
    e.add(Layer::from_bytes(slice.clone(), baseline(1)).unwrap());
    let slice_ids =
        IdMap::open(&Pack::open(&slice).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let country = (0..slice_ids.len())
        .map(|r| e.get(slice_ids.id(r).unwrap()).unwrap().unwrap())
        .find(|p| p.kind == Some(PlaceKind::Country))
        .unwrap();
    let at = country.point.to_e7();
    let mut out = Vec::new();
    e.missing_packs(at, LOCATOR_RADIUS_M, &mut out).unwrap();
    assert_eq!(out.iter().map(|c| c.id).collect::<Vec<_>>(), [country.id]);
    let code = country.id.namespace().country().unwrap();
    e.add(Layer::from_bytes(pack, LayerOptions::default()).unwrap());
    let l = e.get(country.id).unwrap().unwrap().layer;
    assert_eq!(e.layer(l).unwrap().address().as_bytes(), code);
    e.missing_packs(at, LOCATOR_RADIUS_M, &mut out).unwrap();
    assert!(out.is_empty(), "{out:?}");
    // Every installed layer still answers `countries_near` for the country, once.
    e.countries_near(at, LOCATOR_RADIUS_M, &mut out).unwrap();
    assert_eq!(out.iter().filter(|c| c.id == country.id).count(), 1);
}
