//! Radius queries and the admin hierarchy (step 4.1): `within_radius` is `reverse` cut at the
//! radius, `nearby` is `reverse` without the place itself, and `parent`, `ancestors` and
//! `children` follow the pack's HIERARCHY.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    reason = "tests: failures panic with their message; rows fit u32"
)]

mod common;

use common::{Rng, synthetic};
use teigeodata_engine::{Engine, Layer, LayerOptions, Near, Place, Scratch};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};

/// Row `r`'s parent in the test tree: `(r - 1) / 4`, row 0 the root.
fn parent_of(r: usize) -> Option<usize> {
    r.checked_sub(1).map(|r| r / 4)
}

/// `pack` with a HIERARCHY of the test tree added.
fn with_hierarchy(pack: &[u8], n: usize) -> Vec<u8> {
    let p = Pack::open(pack).unwrap();
    let parents: Vec<Option<u32>> = (0..n).map(|r| parent_of(r).map(|x| x as u32)).collect();
    let body = teigeodata_format::hierarchy::encode(&parents).unwrap();
    let mut sections: Vec<(Kind, Encoding, u64, &[u8])> = p
        .sections()
        .filter(|s| s.kind() != Kind::BLOCK_HASHES && s.kind() != Kind::HIERARCHY)
        .map(|s| (s.kind(), s.encoding(), s.item_count(), s.bytes()))
        .collect();
    sections.push((Kind::HIERARCHY, Encoding::TABLE, n as u64, &body));
    let m = p.manifest();
    let mut w = Writer::new(m.address(), m.component(), m.release()).unwrap();
    if let Some(b) = m.get(Tag::BBOX) {
        w.manifest(Tag::BBOX, b).unwrap();
    }
    w.pack(&sections, None).unwrap()
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
    let model = synthetic(n, 4);
    let mut e = Engine::new();
    e.add(Layer::from_bytes(with_hierarchy(&model.encode(), n), LayerOptions::default()).unwrap());
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
        let kids: Vec<u64> = (row * 4 + 1..(row * 4 + 5).min(n))
            .map(|c| model.ids[c].get())
            .collect();
        assert_eq!(
            down.iter().map(|x| x.id.get()).collect::<Vec<_>>(),
            kids,
            "row {row}"
        );
    }
    // A pack without a hierarchy answers none.
    let mut flat = Engine::new();
    flat.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let p = flat.get(model.ids[9]).unwrap().unwrap();
    flat.ancestors(&p, &mut up).unwrap();
    flat.children(&p, &mut down).unwrap();
    assert_eq!(
        (up.len(), down.len(), flat.parent(&p).unwrap()),
        (0, 0, None)
    );
}
