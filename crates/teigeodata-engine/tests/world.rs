//! The world part of the embedded baseline (`forge world`, D-147), alone and under a country's
//! slice and pack. Ignored: it needs the built files.
//!
//! ```text
//! TGD_WORLD=…/world-baseline-main.tgd TGD_PROBES=…/world-probes-main.tsv
//! [TGD_BASELINE=…/np-baseline-main.tgd TGD_PACK=…/np-places-main.tgd]
//! [TGD_EMBED=…/baseline-main.tgd] TEIGEODATA_BASELINE=…/baseline-main.tgd
//!   cargo test --release -p teigeodata-engine --all-features --test world -- --ignored --nocapture
//! ```
//!
//! `--all-features` takes `baseline`, which embeds the file at `TEIGEODATA_BASELINE` (absolute).

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
            // Timezone areas (D-151) are roots; `the_world_part_answers_timezones` checks them.
            Some(PlaceKind::TimezoneArea) => {
                assert!(up.is_empty(), "a timezone area has no parent");
                continue;
            }
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

/// World timezones (D-151): every country and unit has a zone; `tz_at` at a unit's label
/// point (inside it) is the unit's own zone; at a city it is the city's own zone except where
/// the city's unit spans two zones; a pack country's slice is answered by the world's row.
#[test]
#[ignore = "needs TGD_WORLD, TGD_BASELINE"]
fn the_world_part_answers_timezones() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let ids = IdMap::open(&Pack::open(&world).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let mut e = Engine::new();
    e.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let (mut countries, mut units, mut unit_agree) = (0_usize, 0_usize, 0_usize);
    let (mut cities, mut city_agree, mut zones) = (0_usize, 0_usize, 0_usize);
    let t = Instant::now();
    for r in 0..ids.len() {
        let p = e.get(ids.id(r).unwrap()).unwrap().unwrap();
        let own = e.tz_of(&p).unwrap().map(|z| z.id);
        match p.kind {
            Some(PlaceKind::TimezoneArea) => {
                zones += 1;
                continue;
            }
            Some(PlaceKind::Country) => {
                countries += 1;
                assert!(own.is_some(), "{:?}: a country without a zone", p.id);
                continue;
            }
            Some(PlaceKind::Admin) => units += 1,
            Some(PlaceKind::Settlement) => cities += 1,
            k => panic!("unexpected kind {k:?}"),
        }
        assert!(own.is_some(), "{:?}: no zone", p.id);
        let at = e.tz_at(p.point.to_e7()).unwrap().map(|z| z.id);
        match p.kind {
            Some(PlaceKind::Admin) => unit_agree += usize::from(at == own),
            _ => city_agree += usize::from(at == own),
        }
    }
    let per = t.elapsed() / u32::try_from(units + cities).unwrap();
    let (ur, cr) = (
        unit_agree as f64 / units as f64,
        city_agree as f64 / cities as f64,
    );
    println!(
        "{zones} zones, {countries} countries; tz_at agrees with tz_of for {unit_agree} of \
         {units} units ({:.2}%) and {city_agree} of {cities} cities ({:.2}%); {per:?} per pair",
        ur * 100.0,
        cr * 100.0
    );
    assert!(zones > 300 && countries > 200);
    assert!(ur >= 0.99, "units {ur:.4}");
    assert!(cr >= 0.97, "cities {cr:.4}");
    // Under a pack country's slice (no zones of its own), the world answers for it.
    if let Some(slice) = read("TGD_BASELINE") {
        let mut both = e.clone();
        let sl = both.add(Layer::from_bytes(slice.clone(), baseline(1)).unwrap());
        let slice_ids =
            IdMap::open(&Pack::open(&slice).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
        let mut buf = [0_u8; 64];
        let mut names = std::collections::BTreeSet::new();
        for r in 0..slice_ids.len() {
            let p = both.get(slice_ids.id(r).unwrap()).unwrap().unwrap();
            assert_eq!(p.layer, sl);
            let z = both.tz_of(&p).unwrap().expect("a slice place's zone");
            assert_eq!(
                both.tz_at(p.point.to_e7()).unwrap().map(|z| z.id),
                Some(z.id)
            );
            names.insert(
                both.name(&z, &mut buf)
                    .unwrap()
                    .unwrap_or_default()
                    .to_owned(),
            );
        }
        println!("the slice's places are in {names:?}");
        assert_eq!(names.len(), 1, "a one-zone country: {names:?}");
    }
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

/// The cost of shipping slices as layers (step 4.2): the world part alone, then under 1, 8, 64
/// and 200 copies of a country's slice, each its own baseline layer. Every query runs at the
/// world's places (most far from the slices, as most of the world is from any one slice), so
/// area queries show the layers' box pruning and `search` the per-layer cost it cannot prune.
#[test]
#[ignore = "needs TGD_WORLD and TGD_BASELINE; a measurement"]
fn the_cost_of_slices_as_layers() {
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let slice = read("TGD_BASELINE").expect("TGD_BASELINE");
    let ids = IdMap::open(&Pack::open(&world).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
    let mut probe = Engine::new();
    probe.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    let (mut names, mut points) = (Vec::new(), Vec::new());
    let mut buf = [0_u8; 512];
    let step = (ids.len() / 500).max(1);
    for r in (0..ids.len()).step_by(step) {
        let p = probe.get(ids.id(r).unwrap()).unwrap().unwrap();
        if let Some(n) = probe.name(&p, &mut buf).unwrap() {
            names.push(n.to_owned());
        }
        points.push(p.point);
    }
    println!("{} names, {} points", names.len(), points.len());
    println!("| slices | search k=10 (ns) | reverse k=10 (ns) | admin_at (ns) | tz_at (ns) |");
    for n in [0_usize, 1, 8, 64, 200] {
        let mut e = Engine::new();
        e.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
        for _ in 0..n {
            e.add(Layer::from_bytes(slice.clone(), baseline(1)).unwrap());
        }
        let (mut s, mut hits, mut near, mut units) =
            (Scratch::default(), Vec::new(), Vec::new(), Vec::new());
        let per = |t: Instant, k: usize| t.elapsed().as_nanos() as f64 / k as f64;
        for q in &names {
            e.search(q, 10, &mut s, &mut hits).unwrap();
        }
        let t = Instant::now();
        for q in &names {
            e.search(q, 10, &mut s, &mut hits).unwrap();
        }
        let search = per(t, names.len());
        let t = Instant::now();
        for &p in &points {
            e.reverse(p, 10, &mut s, &mut near).unwrap();
        }
        let reverse = per(t, points.len());
        let t = Instant::now();
        for &p in &points {
            e.admin_at(p.to_e7(), &mut s, &mut units).unwrap();
        }
        let admin = per(t, points.len());
        let t = Instant::now();
        for &p in &points {
            e.tz_at(p.to_e7()).unwrap();
        }
        let tz = per(t, points.len());
        println!("| {n} | {search:.0} | {reverse:.0} | {admin:.0} | {tz:.0} |");
    }
}

/// The shipped baseline (`forge embed`, `TGD_EMBED`) is one layer that answers as its parts do
/// as layers, the slice (`TGD_BASELINE`) above the world part (`TGD_WORLD`): for every ID, the
/// same place, name, chain and zone; for every sampled name, the same hits; at every place, the
/// same units and zone. The last holds because the world's neighbours meet each pack country on
/// its slice's own line (D-154); before that, 3 border places differed (D-153). Then the cost
/// of each.
#[test]
#[ignore = "needs TGD_EMBED, TGD_WORLD and TGD_BASELINE"]
fn the_shipped_baseline_answers_as_its_parts() {
    let embed = read("TGD_EMBED").expect("TGD_EMBED");
    let world = read("TGD_WORLD").expect("TGD_WORLD");
    let slice = read("TGD_BASELINE").expect("TGD_BASELINE");
    let mut one = Engine::new();
    one.add(Layer::from_bytes(embed, baseline(0)).unwrap());
    let mut two = Engine::new();
    two.add(Layer::from_bytes(world.clone(), baseline(0)).unwrap());
    two.add(Layer::from_bytes(slice.clone(), baseline(1)).unwrap());
    let mut ids: Vec<PlaceId> = Vec::new();
    for bytes in [&world, &slice] {
        let map = IdMap::open(&Pack::open(bytes).unwrap().section(Kind::IDMAP).unwrap()).unwrap();
        ids.extend((0..map.len()).map(|r| map.id(r).unwrap()));
    }
    ids.sort_unstable();
    ids.dedup();
    let (mut s, mut up_one, mut up_two, mut units_one, mut units_two) = (
        Scratch::default(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let (mut buf_one, mut buf_two) = ([0_u8; 512], [0_u8; 512]);
    let (mut names, mut points, mut off) = (Vec::new(), Vec::new(), Vec::new());
    for (i, &id) in ids.iter().enumerate() {
        let x = one.get(id).unwrap().unwrap();
        let y = two.get(id).unwrap().unwrap();
        assert_eq!(
            (x.point, x.kind, x.admin_level, x.flags),
            (y.point, y.kind, y.admin_level, y.flags),
            "{id:?}"
        );
        let name = one.name(&x, &mut buf_one).unwrap().map(str::to_owned);
        assert_eq!(
            name.as_deref(),
            two.name(&y, &mut buf_two).unwrap(),
            "{id:?}"
        );
        one.ancestors(&x, &mut up_one).unwrap();
        two.ancestors(&y, &mut up_two).unwrap();
        let chain = |v: &[teigeodata_engine::Place]| v.iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(chain(&up_one), chain(&up_two), "the chain of {id:?}");
        let zone = |e: &Engine, p| {
            e.tz_of(p)
                .unwrap()
                .map(|z| e.name(&z, &mut [0; 512]).unwrap().map(str::to_owned))
        };
        assert_eq!(zone(&one, &x), zone(&two, &y), "the zone of {id:?}");
        let at = x.point.to_e7();
        one.admin_at(at, &mut s, &mut units_one).unwrap();
        two.admin_at(at, &mut s, &mut units_two).unwrap();
        let mut set_one = chain(&units_one);
        let mut set_two = chain(&units_two);
        set_one.sort_unstable();
        set_two.sort_unstable();
        let tz = |e: &Engine| {
            e.tz_at(at)
                .unwrap()
                .map(|z| e.name(&z, &mut [0; 512]).unwrap().map(str::to_owned))
        };
        if set_one != set_two || tz(&one) != tz(&two) {
            off.push((id, set_one, set_two));
        }
        if i % 50 == 0 {
            names.extend(name);
            points.push(x.point);
        }
    }
    for o in off.iter().take(10) {
        println!("units or zone differ: {o:?}");
    }
    assert!(
        off.is_empty(),
        "{} of {} places differ",
        off.len(),
        ids.len()
    );
    let (mut hits_one, mut hits_two) = (Vec::new(), Vec::new());
    let mut same = 0;
    for q in &names {
        one.search(q, 10, &mut s, &mut hits_one).unwrap();
        two.search(q, 10, &mut s, &mut hits_two).unwrap();
        let top = |h: &[teigeodata_engine::Hit]| {
            h.iter().map(|x| (x.place.id, x.score)).collect::<Vec<_>>()
        };
        if top(&hits_one) == top(&hits_two) {
            same += 1;
        } else {
            println!("{q:?}: {:?} vs {:?}", top(&hits_one), top(&hits_two));
        }
    }
    println!(
        "{} IDs; {same} of {} searches the same",
        ids.len(),
        names.len()
    );
    assert_eq!(same, names.len(), "search answers as the layers do");
    let mut cost = |e: &Engine| {
        let t = Instant::now();
        for q in &names {
            e.search(q, 10, &mut s, &mut hits_one).unwrap();
        }
        let search = t.elapsed().as_nanos() as f64 / names.len() as f64;
        let t = Instant::now();
        for &p in &points {
            e.admin_at(p.to_e7(), &mut s, &mut units_one).unwrap();
        }
        (search, t.elapsed().as_nanos() as f64 / points.len() as f64)
    };
    let (c1, c2) = (cost(&one), cost(&two));
    println!(
        "one layer: search {:.0} ns, admin_at {:.0} ns; two layers: {:.0} ns, {:.0} ns",
        c1.0, c1.1, c2.0, c2.1
    );
}
