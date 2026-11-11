//! A place's extent (D-194): `bbox_of` gives the stored box of its outline, `boundary` the
//! outline at a level of detail; both come from the first layer that holds an outline of the
//! place, so a part pack holding a unit only as a stub falls through to the country pack.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names,
    reason = "tests: failures panic with their message; values are small"
)]

mod common;

use common::{EAST, Model, NORTH, SOUTH, WEST, synthetic};
use teigeodata_core::{E7, PlaceKind, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Outline, Rect};
use teigeodata_format::arcs::{self, ArcColumns, PINNED};
use teigeodata_format::poly_index;
use teigeodata_format::polygons::{self, PolygonInput, arc_ref};
use teigeodata_format::{Kind, Pack};

/// Half the side of a district, 1e-7°: the synthetic country is 4 × 2 districts of 2° × 2°.
const HALF: i32 = (EAST - WEST) / 8;

fn at(lat: i32, lon: i32) -> Point<E7> {
    Point::<E7>::from_units(lat, lon).unwrap()
}

/// A closed ring around `[w, s, e, n]` with a point halfway along the south edge that only full
/// resolution keeps, and its levels.
fn ring(w: i32, s: i32, e: i32, n: i32) -> (Vec<Point<E7>>, Vec<u8>) {
    let mid = w + (e - w) / 2;
    (
        vec![at(s, w), at(s, mid), at(s, e), at(n, e), at(n, w), at(s, w)],
        vec![PINNED, 0, PINNED, PINNED, PINNED, PINNED],
    )
}

/// The synthetic model with outlines: the country (with a hole) and, unless `country_only`, its
/// 8 districts. Returns the model and each outlined row with its exact box `[w, s, e, n]` on
/// the 1e-7° grid.
fn outlined(country_only: bool) -> (Model, Vec<(u32, [i32; 4])>) {
    let mut m = synthetic(300, 7);
    // The admin index stays off: it was written for these regions without arcs.
    m.admin = None;
    let mut shapes = Vec::new();
    for (r, (&kind, &level)) in m.kinds.iter().zip(&m.levels).enumerate() {
        let r = r as u32;
        if kind == PlaceKind::Country {
            shapes.push((r, [WEST, SOUTH, EAST, NORTH]));
        } else if kind == PlaceKind::Admin && level == 1 && !country_only {
            let p = m.points[r as usize];
            let (lat, lon) = (p.lat() * 10, p.lon() * 10);
            shapes.push((r, [lon - HALF, lat - HALF, lon + HALF, lat + HALF]));
        }
    }
    // POLYGONS order: level 0, then level 1, each by row.
    shapes.sort_by_key(|&(r, _)| (m.levels[r as usize], r));
    let mut rings: Vec<(Vec<Point<E7>>, Vec<u8>)> = shapes
        .iter()
        .map(|&(_, [w, s, e, n])| ring(w, s, e, n))
        .collect();
    // A hole in the country: arc after the polygons' own.
    let c = i32::midpoint(SOUTH, NORTH);
    rings.push(ring(
        WEST + HALF,
        c - HALF / 4,
        WEST + HALF + HALF / 2,
        c + HALF / 4,
    ));
    let pts: Vec<&[Point<E7>]> = rings.iter().map(|r| r.0.as_slice()).collect();
    let lvs: Vec<&[u8]> = rings.iter().map(|r| r.1.as_slice()).collect();
    let tol = [100, 200, 400, 800, 1_600, 3_200];
    let arcs_body = arcs::encode(&ArcColumns {
        arcs: &pts,
        levels: &lvs,
        tolerances_cm: &tol,
    })
    .unwrap();
    let hole = shapes.len() as u32;
    let refs: Vec<Vec<u32>> = (0..shapes.len() as u32)
        .map(|a| vec![arc_ref(a, false)])
        .collect();
    let hole_ref = [arc_ref(hole, false)];
    let ring_lists: Vec<Vec<&[u32]>> = refs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut v = vec![r.as_slice()];
            if i == 0 {
                v.push(&hole_ref[..]);
            }
            v
        })
        .collect();
    let parts: Vec<[&[&[u32]]; 1]> = ring_lists.iter().map(|r| [r.as_slice()]).collect();
    let input: Vec<PolygonInput<'_>> = shapes
        .iter()
        .zip(&parts)
        .map(|(&(row, [w, s, e, n]), parts)| PolygonInput {
            row,
            parts: &parts[..],
            label: at(s + (n - s) / 2, w + (e - w) / 2),
            area_ha: 1,
        })
        .collect();
    let groups: &[u32] = if country_only { &[0] } else { &[0, 1] };
    let poly_body = polygons::encode(&input, groups).unwrap();
    let boxes: Vec<Rect> = shapes
        .iter()
        .map(|&(_, [w, s, e, n])| {
            let mut b = Rect::around_e7(s, w);
            b.grow(&Rect::around_e7(n, e));
            b
        })
        .collect();
    let ix_body = poly_index::encode(&boxes, 4).unwrap();
    let k = shapes.len() as u64;
    m.arcs = Some((arcs_body, k + 1));
    m.raw = vec![
        (Kind::POLYGONS, k, poly_body),
        (Kind::POLY_INDEX, k, ix_body),
    ];
    (m, shapes)
}

fn engine(packs: &[Vec<u8>]) -> Engine {
    let mut e = Engine::new();
    for p in packs {
        e.add(Layer::from_bytes(p.clone(), LayerOptions::default()).unwrap());
    }
    e
}

/// Whether `b` holds the exact box `[w, s, e, n]` (1e-7°) and strays from it by at most `slack`
/// on the 1e-6° grid.
fn frames(b: &Rect, [w, s, e, n]: [i32; 4], slack: i32) -> bool {
    let (s, n) = (s.div_euclid(10), (n + 9).div_euclid(10));
    let (w, e) = (w.div_euclid(10), (e + 9).div_euclid(10));
    b.min_lat <= s
        && b.max_lat >= n
        && b.min_lon <= w
        && b.max_lon >= e
        && s - b.min_lat <= slack
        && b.max_lat - n <= slack
        && w - b.min_lon <= slack
        && b.max_lon - e <= slack
}

#[test]
fn a_place_is_framed_by_its_box_and_drawn_at_each_level() {
    let (m, shapes) = outlined(false);
    let whole = m.encode();
    Pack::open(&whole).unwrap();
    let e = engine(std::slice::from_ref(&whole));
    // Two quanta of the root box (the country, 8° wide) on the 1e-6° grid.
    let slack = 2 * (8_000_000 / 65_535 + 1);
    let mut out = Outline::default();
    for &(row, exact) in &shapes {
        let p = e.get(m.ids[row as usize]).unwrap().unwrap();
        let b = e.bbox_of(&p).unwrap().unwrap();
        assert!(frames(&b, exact, slack), "{b:?} for {exact:?}");
        // Full resolution keeps the mid-edge point; level 1 drops it; a level past 6 reads 6.
        for (lod, len, tol) in [(0, 6, 0), (1, 5, 100), (9, 5, 3_200)] {
            e.boundary(&p, lod, &mut out).unwrap().unwrap();
            let outer = &out.rings[0];
            assert_eq!((outer.part, outer.hole), (0, false));
            let pts = &out.points[outer.points.clone()];
            assert_eq!(pts.len(), len, "lod {lod}");
            assert_eq!(pts.first(), pts.last(), "closed");
            assert_eq!(out.tolerance_cm, tol);
        }
        let country = p.kind == Some(PlaceKind::Country);
        assert_eq!(out.rings.len(), if country { 2 } else { 1 });
        if country {
            assert!(out.rings[1].hole);
            assert_eq!(out.rings[1].points.end, out.points.len());
        }
    }
    // A settlement has no outline: no box, no outline, and the buffer is emptied.
    let r = m
        .kinds
        .iter()
        .position(|&k| k == PlaceKind::Settlement)
        .unwrap();
    let p = e.get(m.ids[r]).unwrap().unwrap();
    assert_eq!(e.bbox_of(&p).unwrap(), None);
    assert_eq!(e.boundary(&p, 0, &mut out).unwrap(), None);
    assert!(out.points.is_empty() && out.rings.is_empty());
}

#[test]
fn a_stub_in_a_part_falls_through_to_the_pack_with_the_outline() {
    let (m, shapes) = outlined(false);
    let whole = m.encode();
    // Parts by row parity: each holds the districts only as rows, with no outlines.
    let mut packs = m.split(2, |r, _| (r % 2) as usize);
    // A part with outlines, but not the districts': every row, the country's outline only.
    let (mut c, _) = outlined(true);
    c.address = "np-c".into();
    packs.push(c.encode());
    packs.push(whole);
    let e = engine(&packs);
    let id_of = |a: &str| {
        e.layers()
            .find(|l| l.address() == a)
            .map(Layer::id)
            .unwrap()
    };
    let (whole_id, c_id) = (id_of("np"), id_of("np-c"));
    let mut out = Outline::default();
    for &(row, exact) in &shapes {
        let p = e.get(m.ids[row as usize]).unwrap().unwrap();
        assert_ne!(p.layer, whole_id, "a part answers the place itself");
        let b = e.bbox_of(&p).unwrap().unwrap();
        assert!(frames(&b, exact, 2 * (8_000_000 / 65_535 + 1)));
        // The country's outline is in `np-c`, above the whole pack; the districts' only below.
        let from = if p.kind == Some(PlaceKind::Country) {
            c_id
        } else {
            whole_id
        };
        assert_eq!(e.boundary(&p, 0, &mut out).unwrap(), Some(from));
    }
}
