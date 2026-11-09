//! The metamorphic gate (ADR-0011, the engine note §5): splitting a pack's places into several
//! packs, overlapping them, or adding them in another order never changes an answer. Also the
//! precedence rules across releases, with tombstones and redirects in the newer layer.
//!
//! `TGD_PACK=<path to a .tgd>` runs the same gate on a real pack (`--ignored`, release mode).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::panic,
    clippy::many_single_char_names,
    reason = "tests: failures panic with their message; values are small"
)]

mod common;

use common::{EAST, Model, NORTH, Rng, SOUTH, WEST, synthetic};
use teigeodata_core::{E6, E7, Namespace, PlaceId, PlaceKind, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Place, Scratch};

/// Everything about a place that does not depend on which pack holds it.
type Key = (
    u64,
    i32,
    i32,
    Option<PlaceKind>,
    u8,
    u32,
    Option<i16>,
    String,
);

fn key(e: &Engine, p: &Place) -> Key {
    let mut buf = [0_u8; 1024];
    let name = e.name(p, &mut buf).unwrap().unwrap_or_default().to_owned();
    (
        p.id.get(),
        p.point.lat(),
        p.point.lon(),
        p.kind,
        p.admin_level,
        p.flags,
        p.elevation,
        name,
    )
}

fn engine(packs: &[Vec<u8>]) -> Engine {
    let mut e = Engine::new();
    for p in packs {
        e.add(Layer::from_bytes(p.clone(), LayerOptions::default()).unwrap());
    }
    e
}

/// The probes every engine answers.
struct Probes {
    ids: Vec<PlaceId>,
    points: Vec<Point<E7>>,
    queries: Vec<String>,
}

fn probes(model: &Model, seed: u64, n: usize) -> Probes {
    let mut r = Rng(seed | 1);
    let ns = Namespace::from_country(*b"np").unwrap();
    let mut ids: Vec<PlaceId> = (0..n)
        .map(|_| model.ids[r.below(model.ids.len() as u64) as usize])
        .collect();
    ids.extend(model.redirects.iter().map(|p| p.0));
    ids.extend((0..20).map(|_| PlaceId::new(ns, 1 + r.below(1 << 40)).unwrap()));
    ids.push(PlaceId::new(Namespace::from_country(*b"in").unwrap(), 5).unwrap());
    let (s, no, w, ea) = (model_box(model)).unwrap_or((SOUTH, NORTH, WEST, EAST));
    let mut points: Vec<Point<E7>> = (0..n)
        .map(|_| {
            // Mostly inside the box, some just outside it.
            let pad = (no - s) / 10;
            let lat = s - pad + r.below((no - s + 2 * pad) as u64) as i32;
            let lon = w - pad + r.below((ea - w + 2 * pad) as u64) as i32;
            Point::from_units(
                lat.clamp(-900_000_000, 900_000_000),
                lon.clamp(-1_800_000_000, 1_800_000_000),
            )
            .unwrap()
        })
        .collect();
    // Exactly on places (distance 0, ties between copies of a place in several packs).
    points.extend(
        (0..n / 4).map(|_| model.points[r.below(model.points.len() as u64) as usize].to_e7()),
    );
    let mut queries = Vec::new();
    for _ in 0..n {
        let row = r.below(model.names.len() as u64) as usize;
        let Some(first) = model.names[row].first() else {
            continue;
        };
        let t = first.text.clone();
        let chars: Vec<char> = t.chars().collect();
        queries.push(t.clone());
        queries.push(t.to_uppercase());
        queries.push(t.replace(' ', ""));
        let cut = 3 + r.below(3) as usize;
        queries.push(chars.iter().take(cut).collect());
        if chars.len() > 4 {
            let mut typo = chars.clone();
            let at = 1 + r.below(chars.len() as u64 - 1) as usize;
            typo[at] = if typo[at] == 'a' { 'e' } else { 'a' };
            queries.push(typo.into_iter().collect());
        }
    }
    queries.extend(["", "  ", "x", "zzzzzz", "ka"].map(String::from));
    Probes {
        ids,
        points,
        queries,
    }
}

fn model_box(model: &Model) -> Option<(i32, i32, i32, i32)> {
    let b = model.bbox.as_ref()?;
    let v = |i: usize| i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    Some((v(0), v(2), v(1), v(3)))
}

#[derive(Debug, PartialEq)]
struct Answers {
    get: Vec<Option<Key>>,
    reverse: Vec<Vec<(Key, u64)>>,
    admin: Vec<Vec<Key>>,
    tz: Vec<Option<Key>>,
    search: Vec<Vec<(Key, i32)>>,
}

fn answers(e: &Engine, p: &Probes) -> Answers {
    let mut s = Scratch::default();
    let (mut near, mut admin, mut hits) = (Vec::new(), Vec::new(), Vec::new());
    let get = p
        .ids
        .iter()
        .map(|&id| e.get(id).unwrap().map(|x| key(e, &x)))
        .collect();
    let mut reverse = Vec::new();
    let mut admins = Vec::new();
    let mut tz = Vec::new();
    for (i, &at) in p.points.iter().enumerate() {
        let k = [1, 7, 30][i % 3];
        e.reverse(at.to_e6(), k, &mut s, &mut near).unwrap();
        reverse.push(
            near.iter()
                .map(|n| (key(e, &n.place), n.meters.to_bits()))
                .collect(),
        );
        e.admin_at(at, &mut s, &mut admin).unwrap();
        admins.push(admin.iter().map(|x| key(e, x)).collect());
        tz.push(e.tz_at(at).unwrap().map(|x| key(e, &x)));
    }
    let mut search = Vec::new();
    for (i, q) in p.queries.iter().enumerate() {
        let k = [1, 5, 20][i % 3];
        e.search(q, k, &mut s, &mut hits).unwrap();
        search.push(hits.iter().map(|h| (key(e, &h.place), h.score)).collect());
    }
    Answers {
        get,
        reverse,
        admin: admins,
        tz,
        search,
    }
}

/// Splits `model` into `parts` packs by a hash of each ID, and checks every answer against the
/// whole pack, in the split order, reversed, and with one part added twice under another name.
fn gate(model: &Model, whole: &[u8], parts: usize, seed: u64, n: usize) {
    let p = probes(model, seed, n);
    let want = answers(&engine(&[whole.to_vec()]), &p);
    // The answers are not trivially empty.
    assert!(want.get.iter().filter(|g| g.is_some()).count() > n / 2);
    assert!(want.reverse.iter().all(|r| !r.is_empty()));
    assert!(want.admin.iter().any(|a| a.len() > 1));
    assert!(want.tz.iter().any(Option::is_some));
    assert!(want.search.iter().filter(|h| !h.is_empty()).count() > n);

    let salt = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let split = model.split(parts, |_, id| {
        ((id.get() ^ salt).wrapping_mul(0xff51_afd7_ed55_8ccd) >> 33) as usize % parts
    });
    let mut packs = split.clone();
    assert_eq!(answers(&engine(&packs), &p), want, "{parts} parts");
    packs.reverse();
    assert_eq!(
        answers(&engine(&packs), &p),
        want,
        "{parts} parts, reversed"
    );
    // An overlapping copy of one part, under another address: its places are hidden by the
    // original (same release, the address breaks the tie) or hide it; answers stay the same.
    let mut copy = Model::from_pack(&split[0]);
    copy.address = "np-copy".into();
    packs.push(copy.encode());
    assert_eq!(
        answers(&engine(&packs), &p),
        want,
        "{parts} parts and a copy"
    );
}

#[test]
fn splitting_and_reordering_never_change_an_answer() {
    for (seed, n, parts) in [(1, 600, 2), (7, 1500, 3), (42, 900, 5)] {
        let model = synthetic(n, seed);
        let whole = model.encode();
        // The model round-trips through a pack.
        let again = Model::from_pack(&whole);
        assert_eq!(again.ids, model.ids);
        assert_eq!(again.keys, model.keys);
        assert_eq!(again.encode(), whole);
        gate(&model, &whole, parts, seed, 150);
    }
}

#[test]
fn a_newer_release_wins_with_its_tombstones_and_redirects() {
    let v1 = synthetic(800, 3);
    let mut r = Rng(99);
    // v2: one place moved, one tombstoned, one merged into another; a new release.
    let mut v2 = v1.clone();
    v2.release = "2026.12.0".into();
    let settlement = |r: &mut Rng| loop {
        let i = r.below(v1.ids.len() as u64) as usize;
        if v1.kinds[i] == PlaceKind::Settlement && !v1.linked_rows().contains(&(i as u32)) {
            return i;
        }
    };
    let (moved, dead, merged) = (settlement(&mut r), settlement(&mut r), settlement(&mut r));
    let into = settlement(&mut r);
    assert!(moved != dead && dead != merged && merged != into && moved != merged);
    v2.elevations[moved] = Some(1234);
    let (dead_id, merged_id, into_id) = (v1.ids[dead], v1.ids[merged], v1.ids[into]);
    // The forge flattens chains: v1's redirects to the dead or merged place follow them.
    for r in &mut v2.redirects {
        if r.1 == Some(dead_id) {
            r.1 = None;
        } else if r.1 == Some(merged_id) {
            r.1 = Some(into_id);
        }
    }
    let keep: Vec<u32> = (0..v1.ids.len() as u32)
        .filter(|&i| i as usize != dead && i as usize != merged)
        .collect();
    let mut v2 = v2.subset("np", &keep, |_| true);
    v2.redirects.push((dead_id, None));
    v2.redirects.push((merged_id, Some(into_id)));
    let (p1, p2) = (v1.encode(), v2.encode());

    for order in [[&p1, &p2], [&p2, &p1]] {
        let e = engine(&[order[0].clone(), order[1].clone()]);
        let top = e.layers().next().unwrap();
        assert_eq!(top.release(), "2026.12.0");
        let newer = top.id();
        // The tombstone hides the place v1 still has; the redirect leads to the survivor.
        assert_eq!(e.get(dead_id).unwrap(), None);
        let got = e.get(merged_id).unwrap().unwrap();
        assert_eq!((got.id, got.layer), (into_id, newer));
        // The moved place answers from v2 only.
        let m = e.get(v1.ids[moved]).unwrap().unwrap();
        assert_eq!((m.elevation, m.layer), (Some(1234), newer));
        // Reverse at the dead and merged places never returns them from v1.
        let mut s = Scratch::default();
        let mut out = Vec::new();
        for row in [dead, merged] {
            e.reverse(v1.points[row], 5, &mut s, &mut out).unwrap();
            assert!(out.iter().all(|n| n.place.id != v1.ids[row]));
            assert!(out.iter().all(|n| n.place.layer == newer));
        }
        // Every answer is the one v2 alone gives.
        let p = probes(&v1, 5, 100);
        assert_eq!(
            answers(&e, &p),
            answers(&engine(std::slice::from_ref(&p2)), &p)
        );
    }
}

/// The differential half of the gate: a base pack under a small newer pack that tombstones the
/// places nearest a few points answers exactly as one compacted pack without them. A layer
/// whose nearest places are hidden must be asked again for more.
#[test]
fn a_delta_layer_matches_the_compacted_pack() {
    let base = synthetic(1200, 31);
    let mut r = Rng(77);
    let linked = base.linked_rows();
    let mut dead = std::collections::BTreeSet::new();
    let mut centres = Vec::new();
    for _ in 0..6 {
        let c = base.points[r.below(base.points.len() as u64) as usize];
        centres.push(c);
        let mut by_distance: Vec<(i64, u32)> = (0..base.ids.len() as u32)
            .filter(|row| !linked.contains(row))
            .map(|row| {
                let p = base.points[row as usize];
                let (dy, dx) = (i64::from(p.lat() - c.lat()), i64::from(p.lon() - c.lon()));
                (dy * dy + dx * dx, row)
            })
            .collect();
        by_distance.sort_unstable();
        dead.extend(by_distance.iter().take(25).map(|x| x.1));
    }
    let tombs: Vec<_> = dead
        .iter()
        .map(|&row| (base.ids[row as usize], None))
        .collect();
    // The newer layer: a few unchanged places, the linked ones, and the tombstones.
    let few: Vec<u32> = (0..base.ids.len() as u32)
        .filter(|row| linked.contains(row) || (row % 50 == 7 && !dead.contains(row)))
        .collect();
    let mut delta = base.subset("np-delta", &few, |_| false);
    delta.release = "2026.12.0".into();
    delta.redirects = tombs.clone();
    let keep: Vec<u32> = (0..base.ids.len() as u32)
        .filter(|row| !dead.contains(row))
        .collect();
    let mut compacted = base.subset("np", &keep, |_| true);
    compacted.redirects.extend(tombs);
    let mut p = probes(&base, 9, 120);
    p.points.extend(centres.iter().map(|c| c.to_e7()));
    let want = answers(&engine(&[compacted.encode()]), &p);
    for packs in [
        [base.encode(), delta.encode()],
        [delta.encode(), base.encode()],
    ] {
        assert_eq!(answers(&engine(&packs), &p), want);
    }
    // The centres' nearest answers are full: the base layer was asked for more than k.
    let e = engine(&[base.encode(), delta.encode()]);
    let (mut s, mut out) = (Scratch::default(), Vec::new());
    for c in &centres {
        e.reverse(*c, 30, &mut s, &mut out).unwrap();
        assert_eq!(out.len(), 30);
        assert!(
            out.iter()
                .all(|n| !dead.iter().any(|&d| base.ids[d as usize] == n.place.id))
        );
    }
}

#[test]
fn priority_and_the_baseline_order_layers() {
    let base = synthetic(300, 11);
    let mut low = base.clone();
    low.release = "2027.1.0".into();
    low.address = "np-new".into();
    let options = |priority, baseline| LayerOptions {
        priority,
        baseline,
        ..LayerOptions::default()
    };
    let mut e = Engine::new();
    let a = e.add(Layer::from_bytes(low.encode(), options(0, false)).unwrap());
    let b = e.add(Layer::from_bytes(base.encode(), options(5, false)).unwrap());
    let c = e.add(Layer::from_bytes(base.encode(), options(9, true)).unwrap());
    let order: Vec<_> = e.layers().map(Layer::id).collect();
    // Priority beats the newer release; the baseline is last whatever its priority.
    assert_eq!(order, [b, a, c]);
    assert_eq!(e.tier(c), Some(teigeodata_engine::Tier::Baseline));
    let id = base.ids[0];
    assert_eq!(e.get(id).unwrap().unwrap().layer, b);
    e.remove(b).unwrap();
    assert_eq!(e.get(id).unwrap().unwrap().layer, a);
    e.remove(a).unwrap();
    assert_eq!(e.get(id).unwrap().unwrap().layer, c);
    // The baseline answers a point only where nothing covers it: here it is the only layer.
    let at = Point::<E6>::from_degrees(28.0, 84.0).unwrap().to_e7();
    assert!(e.tz_at(at).unwrap().is_some());
}

/// The gate on a real pack: `TGD_PACK=data/build/pack/np/np-places-main.tgd cargo test
/// --release -p teigeodata-engine --test metamorphic -- --ignored`.
#[test]
#[ignore = "needs TGD_PACK, a real pack; run in release mode"]
fn the_gate_on_a_real_pack() {
    let path = std::env::var("TGD_PACK").expect("set TGD_PACK to a .tgd file");
    let whole = std::fs::read(path).unwrap();
    let model = Model::from_pack(&whole);
    for parts in [2, 3] {
        gate(&model, &whole, parts, 17 + parts as u64, 400);
    }
}
