//! Radius queries and the admin hierarchy (step 4.1): `within_radius` is `reverse` cut at the
//! radius, `nearby` is `reverse` without the place itself, and `parent`, `ancestors` and
//! `children` follow HIERARCHY, across packs by ID (D-143).

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message; rows fit u32"
)]

mod common;

use common::{Model, Rng, synthetic};
use teigeodata_core::PlaceKind;
use teigeodata_engine::{Engine, Layer, LayerOptions, Near, Place, Scratch};

/// Row `r`'s parent in the test tree: `(r - 1) / 4`, row 0 the root.
fn parent_of(r: usize) -> Option<usize> {
    r.checked_sub(1).map(|r| r / 4)
}

/// `model` with the test tree as its HIERARCHY.
fn with_tree(mut model: Model) -> Model {
    model.parents = (0..model.ids.len())
        .map(|r| parent_of(r).map(|x| x as u32))
        .collect();
    model
}

fn ids(near: &[Near]) -> Vec<(u64, u64)> {
    near.iter()
        .map(|n| (n.place.id.get(), n.meters.to_bits()))
        .collect()
}

#[test]
fn a_radius_is_reverse_cut_at_the_radius() {
    let model = synthetic(3_000, 11);
    let mut e = Engine::new();
    e.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let mut s = Scratch::default();
    let (mut all, mut got) = (Vec::new(), Vec::new());
    let mut r = Rng(5);
    let mut nonempty = 0;
    for t in 0..300 {
        let at = model.points[r.below(model.points.len() as u64) as usize];
        e.reverse(at, 200, &mut s, &mut all).unwrap();
        // Radii at, just below and between the distances found, and beyond them all.
        let pick = all[t % all.len()].meters;
        for meters in [pick, pick * 0.999, pick * 1.5, 0.0, 1e9] {
            for k in [1, 7, 200] {
                e.within_radius(at, meters, k, &mut s, &mut got).unwrap();
                let want: Vec<(u64, u64)> = ids(&all)
                    .into_iter()
                    .filter(|&(_, m)| f64::from_bits(m) <= meters)
                    .take(k)
                    .collect();
                // Equal up to rounding at the edge: a place exactly at the radius may fall
                // either side of the haversine cap.
                let edge = |x: &(u64, u64)| (f64::from_bits(x.1) - meters).abs() < 1e-6;
                let core =
                    |v: &[(u64, u64)]| v.iter().filter(|x| !edge(x)).copied().collect::<Vec<_>>();
                assert_eq!(core(&ids(&got)), core(&want), "t {t}, {meters} m, k {k}");
                nonempty += usize::from(got.len() > 1);
            }
        }
        // `nearby` is `reverse` from the place without the place.
        let place = all[0].place;
        e.nearby(&place, 6, &mut s, &mut got).unwrap();
        e.reverse(place.point, 7, &mut s, &mut all).unwrap();
        let want: Vec<u64> = all
            .iter()
            .map(|n| n.place.id.get())
            .filter(|&id| id != place.id.get())
            .take(6)
            .collect();
        assert_eq!(
            got.iter().map(|n| n.place.id.get()).collect::<Vec<_>>(),
            want
        );
    }
    assert!(
        nonempty > 500,
        "the radii are not trivially empty: {nonempty}"
    );
    for bad in [-1.0, f64::NAN] {
        e.within_radius(model.points[0], bad, 5, &mut s, &mut got)
            .unwrap();
        assert!(got.is_empty(), "{bad}");
    }
}

#[test]
fn ancestors_and_children_follow_the_hierarchy() {
    let n = 700;
    let model = with_tree(synthetic(n, 4));
    let n = model.ids.len();
    let mut e = Engine::new();
    e.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let place = |row: usize| -> Place { e.get(model.ids[row]).unwrap().unwrap() };
    let (mut up, mut down) = (Vec::new(), Vec::new());
    for row in (0..n).step_by(7) {
        let p = place(row);
        assert_eq!(p.row as usize, row);
        let mut chain = Vec::new();
        let mut r = row;
        while let Some(q) = parent_of(r) {
            chain.push(model.ids[q].get());
            r = q;
        }
        e.ancestors(&p, &mut up).unwrap();
        assert_eq!(
            up.iter().map(|x| x.id.get()).collect::<Vec<_>>(),
            chain,
            "row {row}"
        );
        assert_eq!(
            e.parent(&p).unwrap().map(|x| x.id.get()),
            chain.first().copied()
        );
        e.children(&p, &mut down).unwrap();
        let mut kids: Vec<u64> = (row * 4 + 1..(row * 4 + 5).min(n))
            .map(|c| model.ids[c].get())
            .collect();
        kids.sort_unstable();
        assert_eq!(
            down.iter().map(|x| x.id.get()).collect::<Vec<_>>(),
            kids,
            "row {row}"
        );
    }
    // A pack without a hierarchy answers none.
    let mut bare = model.clone();
    bare.parents.clear();
    let mut flat = Engine::new();
    flat.add(Layer::from_bytes(bare.encode(), LayerOptions::default()).unwrap());
    let p = flat.get(model.ids[9]).unwrap().unwrap();
    flat.ancestors(&p, &mut up).unwrap();
    flat.children(&p, &mut down).unwrap();
    assert_eq!(
        (up.len(), down.len(), flat.parent(&p).unwrap()),
        (0, 0, None)
    );
}

/// The rows whose parent is the country.
fn districts(model: &Model) -> Vec<u32> {
    let country = model
        .kinds
        .iter()
        .position(|k| *k == PlaceKind::Country)
        .unwrap() as u32;
    (0..model.ids.len() as u32)
        .filter(|&r| model.parents[r as usize] == Some(country))
        .collect()
}

/// The IDs of `p`'s ancestors and children in `e`.
fn family(e: &Engine, p: &Place) -> (Vec<u64>, Vec<u64>) {
    let (mut up, mut down) = (Vec::new(), Vec::new());
    e.ancestors(p, &mut up).unwrap();
    e.children(p, &mut down).unwrap();
    (
        up.iter().map(|x| x.id.get()).collect(),
        down.iter().map(|x| x.id.get()).collect(),
    )
}

#[test]
fn a_pack_without_the_whole_chain_defers_to_one_that_has_it() {
    // A city pack: one district and its settlements, the district a root (no country stub),
    // above the whole country (D-143).
    let model = synthetic(2_000, 9);
    let district = districts(&model)[3] as usize;
    let keep: Vec<u32> = (0..model.ids.len() as u32)
        .filter(|&r| r as usize == district || model.parents[r as usize] == Some(district as u32))
        .collect();
    assert!(keep.len() > 100);
    // The city pack has no region indexes: they link rows it does not hold.
    let mut points_only = model.clone();
    (points_only.zones, points_only.admin) = (None, None);
    points_only.release = "2026.12.0".into();
    let city = points_only.subset("np-city", &keep, |_| false).encode();
    let mut whole = Engine::new();
    whole.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let mut both = Engine::new();
    let city_layer = both.add(Layer::from_bytes(city.clone(), LayerOptions::default()).unwrap());
    both.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let mut alone = Engine::new();
    alone.add(Layer::from_bytes(city, LayerOptions::default()).unwrap());
    for &r in &keep {
        let id = model.ids[r as usize];
        let (w, b) = (
            whole.get(id).unwrap().unwrap(),
            both.get(id).unwrap().unwrap(),
        );
        assert_eq!(b.layer, city_layer, "the newer city pack answers");
        assert_eq!(family(&both, &b), family(&whole, &w));
        // Alone, the city pack knows the chain only up to its district.
        let (up, _) = family(&alone, &alone.get(id).unwrap().unwrap());
        assert_eq!(up.len(), usize::from(r as usize != district));
    }
}

#[test]
fn a_child_moved_by_a_newer_pack_leaves_its_old_parent() {
    let model = synthetic(1_500, 3);
    let d = districts(&model);
    let child = model
        .parents
        .iter()
        .position(|p| p.is_some_and(|q| d.contains(&q)))
        .unwrap();
    let old = model.parents[child].unwrap();
    let new = d.into_iter().find(|&q| q != old).unwrap();
    let mut moved = model.clone();
    moved.release = "2026.12.0".into();
    moved.parents[child] = Some(new);
    let mut e = Engine::new();
    e.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    e.add(Layer::from_bytes(moved.encode(), LayerOptions::default()).unwrap());
    let id = model.ids[child].get();
    let kids = |row: u32| family(&e, &e.get(model.ids[row as usize]).unwrap().unwrap()).1;
    assert!(!kids(old).contains(&id), "still under its old parent");
    assert!(kids(new).contains(&id), "missing under its new parent");
    let c = e.get(model.ids[child]).unwrap().unwrap();
    assert_eq!(e.parent(&c).unwrap().unwrap().id, model.ids[new as usize]);
}
