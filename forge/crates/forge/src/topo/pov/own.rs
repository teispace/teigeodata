//! A land partition owned by one point of view's countries (D-155).
//!
//! The units are another source's (Overture's divisions, drawn from OSM) and come with that
//! source's country code; the countries are Natural Earth's in the point of view the build
//! ships (Nepal's). Natural Earth's lines lie 0.6–10 km from OSM's (the research note §3.4),
//! so the country holding most of a unit is noise near a border: it would give Liechtenstein's
//! municipalities to Austria and Hodoš to Hungary (measured 2026-11-10). So:
//!
//! 1. **Owner:** a unit goes to the country its code names ([`By::Code`]); among several
//!    countries with one code (France and Clipperton) to the one holding most of it, else the
//!    nearest. A code no country of the view holds (Overture's disputed areas `X…`, a state
//!    the view does not recognise, a dependency the view draws inside its sovereign) gives the
//!    unit to the country holding most of it ([`By::Area`]); a unit no country holds any of
//!    (an island off Natural Earth's coarser coast) goes where most of its code's other
//!    units went ([`By::Kin`]: Matsu with Taiwan, Mayotte's communes with France); a code
//!    with none stays unowned.
//! 2. **The view's parts:** where the view gives a unit's land to another country (Western
//!    Sahara, Kashmir, a disputed area's neighbours), that land moves to it. Only parts at
//!    least [`WIDTH`] wide on average move, and the unit's other parts narrower than that move
//!    with them, so the two sources' disagreement along a border never makes a sliver. Each
//!    split's lines are Natural Earth's (the view's border) or the unit's own.
//!
//!    The view is the reference, not Natural Earth's difference from its default view: OSM
//!    draws some claims as neither of Natural Earth's views does (Overture puts all of
//!    Western Sahara in Morocco's regions, Natural Earth's default view only the part west of
//!    the berm), and the unit's own code already is that source's view.
//!
//! A piece of at least [`CHECKED_M2`] kept by its code but mostly outside its country is
//! reported ([`Ownership::offshore`]): a coast Natural Earth draws coarser (Kingman Reef's
//! lagoon is Overture land). A wrong code needs no gate of its own: the view moves every part
//! of it at least [`WIDTH`] wide, and the caller reports every move.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use rayon::prelude::*;

use super::{
    Countries, Input, OverlayRule, Shape, V, area2, bbox, meets, narrower, overlay, rings_of,
    shape, union,
};

/// A piece of at least this many square metres kept by its code is checked against its
/// country's outline (1,000 km²: far above the sources' disagreement along any border).
pub(crate) const CHECKED_M2: f64 = 1e9;

/// The width, in 1e-7°, a part another country holds must reach on average to move (about
/// 15 km). Measured on Overture 2026-10-21.0 against Natural Earth 5.1.2's Nepal view
/// (2026-11-10, 482 parts at least 2 km wide): the two sources' disagreement along borders
/// runs continuously up to 11 km (desert and high-mountain lines: Tibesti, Arunachal, Bhutan),
/// and the narrowest real dispute is 22 km (Azad Kashmir; then Guelmim-Oued Noun's share of
/// Western Sahara, Kafia Kingi). 15 km sits in that gap. Kalapani (3.3 km) is drawn by
/// Nepal's pack (D-154).
pub(crate) const WIDTH: i128 = 1_350_000;

/// How a unit got its country.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum By {
    /// The country its code names.
    Code,
    /// The country holding most of it: its code names none.
    Area,
    /// The country most of its code's other units went to: no country holds any of it.
    Kin,
}

/// A piece of a unit and its country.
#[derive(Debug)]
pub(crate) struct Owned {
    /// The unit's index.
    pub(crate) unit: usize,
    pub(crate) country: usize,
    pub(crate) by: By,
    /// Whether the view moved it from the unit's owner.
    pub(crate) moved: bool,
    /// The unit's own rings when no delta cut it, else the overlay's.
    pub(crate) rings: Vec<Vec<V>>,
}

/// What [`own`] made.
#[derive(Debug, Default)]
pub(crate) struct Ownership {
    /// In unit order; a unit the view does not split is one piece.
    pub(crate) pieces: Vec<Owned>,
    /// Units no country of the view holds any of, with a code it does not hold.
    pub(crate) unowned: Vec<usize>,
    /// Pieces of at least [`CHECKED_M2`] kept by their code that lie mostly outside their
    /// country (and so outside every country by more than [`WIDTH`]): `(unit, country, share
    /// inside it)`.
    pub(crate) offshore: Vec<(usize, usize, f64)>,
}

/// Owns `units` (`Input::code`: the unit's ISO 3166-1 code, lower case) by `view`, whose
/// countries' codes are `iso2` (lower case; several may share one), moving the parts other
/// countries hold at least `width` grid units wide. Deterministic; parallel over units.
pub(crate) fn own(
    view: &[Input<'_>],
    iso2: &[Option<&str>],
    units: &[Input<'_>],
    width: i128,
) -> Ownership {
    let c = Countries::new(view, &[]);
    let mut by_iso: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (k, cc) in iso2.iter().enumerate() {
        if let Some(cc) = cc {
            by_iso.entry(cc).or_default().push(k);
        }
    }
    let owners: Vec<Option<(usize, By)>> =
        units.par_iter().map(|u| owner(&c, &by_iso, u)).collect();
    // Each code's country by the area of its owned units.
    let mut kin: BTreeMap<&str, BTreeMap<usize, i128>> = BTreeMap::new();
    for (u, o) in units.iter().zip(&owners) {
        if let Some((k, _)) = o {
            *kin.entry(u.code).or_default().entry(*k).or_default() +=
                area2(std::slice::from_ref(&shape(u.rings))).abs();
        }
    }
    let kin: BTreeMap<&str, usize> = kin
        .into_iter()
        .filter_map(|(cc, by)| {
            by.into_iter()
                .map(|(k, a)| (a, Reverse(k)))
                .max()
                .map(|(_, Reverse(k))| (cc, k))
        })
        .collect();
    let per_unit: Vec<Option<Vec<Owned>>> = units
        .par_iter()
        .zip(&owners)
        .enumerate()
        .map(|(i, (u, o))| {
            let (country, by) = o.or_else(|| kin.get(u.code).map(|&k| (k, By::Kin)))?;
            Some(
                split(&c, u.rings, country, width)
                    .into_iter()
                    .map(|(k, rings, moved)| Owned {
                        unit: i,
                        country: k,
                        by,
                        moved,
                        rings,
                    })
                    .collect(),
            )
        })
        .collect();
    let mut out = Ownership::default();
    for (i, p) in per_unit.into_iter().enumerate() {
        match p {
            Some(p) => out.pieces.extend(p),
            None => out.unowned.push(i),
        }
    }
    out.offshore = out
        .pieces
        .par_iter()
        .filter(|p| p.by == By::Code && !p.moved)
        .filter(|p| area_m2(&p.rings) >= CHECKED_M2)
        .filter_map(|p| {
            let share = inside(&c, p.country, &p.rings);
            (share < 0.5).then_some((p.unit, p.country, share))
        })
        .collect();
    out
}

/// The unit's country and how it got it; `None` when no country of the view holds any of it.
fn owner(
    c: &Countries<'_>,
    by_iso: &BTreeMap<&str, Vec<usize>>,
    u: &Input<'_>,
) -> Option<(usize, By)> {
    let drawn = shape(u.rings);
    let bounds = bbox(u.rings);
    let overlap = |k: usize| -> i128 {
        c.boxes
            .get(k)
            .filter(|b| meets(b, &bounds))
            .and_then(|_| c.near(k, &bounds))
            .map_or(0, |cs| area2(&overlay(&drawn, &cs, OverlayRule::Intersect)))
    };
    if let Some(ks) = by_iso.get(u.code) {
        if let [k] = ks[..] {
            return Some((k, By::Code));
        }
        let gap = |k: usize| c.boxes.get(k).map_or(i64::MAX, |b| box_gap(b, &bounds));
        return ks
            .iter()
            .map(|&k| (overlap(k), Reverse(gap(k)), Reverse(k)))
            .max()
            .map(|(_, _, Reverse(k))| (k, By::Code));
    }
    (0..c.shapes.len())
        .filter(|&k| c.boxes.get(k).is_some_and(|b| meets(b, &bounds)))
        .map(|k| (overlap(k), Reverse(k)))
        .filter(|&(a, _)| a > 0)
        .max()
        .map(|(_, Reverse(k))| (k, By::Area))
}

/// The gap between two boxes in grid units along x plus along y (0 when they meet).
fn box_gap(a: &[i32; 4], b: &[i32; 4]) -> i64 {
    let axis = |lo1: i32, hi1: i32, lo2: i32, hi2: i32| {
        (i64::from(lo2) - i64::from(hi1))
            .max(i64::from(lo1) - i64::from(hi2))
            .max(0)
    };
    axis(a[0], a[2], b[0], b[2]) + axis(a[1], a[3], b[1], b[3])
}

/// The unit owned by `owner` cut by the other countries of the view: `(country, rings, moved)`
/// pieces, the owner's last (absent when every part moved). As drawn when no other country
/// holds a part at least `width` wide; a piece that is the whole rest keeps its vertices.
fn split(
    c: &Countries<'_>,
    rings: &[Vec<V>],
    owner: usize,
    width: i128,
) -> Vec<(usize, Vec<Vec<V>>, bool)> {
    let bounds = bbox(rings);
    let mut out = Vec::new();
    let mut rest: Vec<Vec<V>> = rings.to_vec();
    for k in (0..c.shapes.len()).filter(|&k| k != owner) {
        if !c.boxes.get(k).is_some_and(|b| meets(b, &bounds)) {
            continue;
        }
        let Some(other) = c.near(k, &bounds) else {
            continue;
        };
        let cur = shape(&rest);
        let inter = overlay(&cur, &other, OverlayRule::Intersect);
        if inter.iter().all(|s| narrower(s, width)) {
            continue;
        }
        let diff = overlay(&cur, &other, OverlayRule::Difference);
        let (mut to, mut keep): (Vec<Shape>, Vec<Shape>) =
            inter.into_iter().partition(|s| !narrower(s, width));
        let (keep_wide, to_narrow): (Vec<Shape>, Vec<Shape>) =
            diff.into_iter().partition(|s| !narrower(s, width));
        to.extend(to_narrow);
        keep.extend(keep_wide);
        if keep.is_empty() {
            out.push((k, std::mem::take(&mut rest), true));
            break;
        }
        out.push((k, rings_of(&union(to)), true));
        rest = rings_of(&union(keep));
    }
    if !rest.is_empty() {
        out.push((owner, rest, false));
    }
    out
}

/// The share of `rings`' area inside country `k` (0 when their boxes do not meet).
fn inside(c: &Countries<'_>, k: usize, rings: &[Vec<V>]) -> f64 {
    let bounds = bbox(rings);
    if !c.boxes.get(k).is_some_and(|b| meets(b, &bounds)) {
        return 0.0;
    }
    let drawn = shape(rings);
    let whole = area2(std::slice::from_ref(&drawn));
    if whole == 0 {
        return 1.0;
    }
    let common = c
        .near(k, &bounds)
        .map_or(0, |cs| area2(&overlay(&drawn, &cs, OverlayRule::Intersect)));
    #[allow(clippy::cast_precision_loss, reason = "a ratio")]
    let share = common as f64 / whole as f64;
    share
}

#[cfg(test)]
/// The mean width of rings read even–odd, `2 · area / perimeter`, in grid units.
pub(crate) fn mean_width(rings: &[Vec<V>]) -> f64 {
    let s = shape(rings);
    let p = super::perimeter(std::slice::from_ref(&s));
    #[allow(clippy::cast_precision_loss, reason = "a measurement")]
    let a = area2(std::slice::from_ref(&s)).abs() as f64;
    if p > 0.0 { a / p } else { 0.0 }
}

/// The geodesic area of rings read even–odd (outer counter-clockwise).
fn area_m2(rings: &[Vec<V>]) -> f64 {
    rings
        .iter()
        .map(|r| crate::topo::measure::ring_area_m2(r))
        .sum::<f64>()
        .abs()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::many_single_char_names,
        reason = "test code; a, b name the countries, o the ownership"
    )]

    use super::*;

    /// One model unit, 10,000 grid units (about 110 m).
    const K: i32 = 10_000;
    /// The moving width: 1.5 model units (a 2 × 10 strip is 1.67 wide on average, a 1 × 10
    /// strip 0.91).
    const W: i128 = 15_000;

    fn rect(w: i32, s: i32, e: i32, n: i32) -> Vec<Vec<V>> {
        let (w, s, e, n) = (w * K, s * K, e * K, n * K);
        vec![vec![(w, s), (e, s), (e, n), (w, n), (w, s)]]
    }

    fn input<'a>(code: &'a str, rings: &'a [Vec<V>]) -> Input<'a> {
        Input { code, rings }
    }

    /// The view: A is [0,10]², B is [10,20]×[0,10]. Unit b1, coded "b", reaches into A over
    /// [7,10] (2.3 wide on average): that part moves to A, B keeps [10,12]. Unit b2 lies in B;
    /// x, coded "x" (no country), lies in B; far, coded "x", lies off every country.
    #[test]
    fn units_go_by_code_then_area_and_the_view_moves_wide_parts() {
        let (a, b) = (rect(0, 0, 10, 10), rect(10, 0, 20, 10));
        let view = [input("AAA", &a), input("BBB", &b)];
        let (b1, b2) = (rect(7, 0, 12, 10), rect(12, 0, 20, 10));
        let far = rect(40, 40, 41, 41);
        let units = [
            input("b", &b1),
            input("b", &b2),
            input("x", &b2),
            input("x", &far),
        ];
        let o = own(&view, &[Some("a"), Some("b")], &units, W);
        // `far` has code x like the unit in B: it goes where x went.
        assert_eq!(o.unowned, Vec::<usize>::new());
        let f = o.pieces.iter().find(|p| p.unit == 3).unwrap();
        assert_eq!((f.country, f.by), (1, By::Kin));
        let of = |u: usize| -> Vec<(usize, bool)> {
            o.pieces
                .iter()
                .filter(|p| p.unit == u)
                .map(|p| (p.country, p.moved))
                .collect()
        };
        assert_eq!(of(0), vec![(0, true), (1, false)]);
        let moved = o.pieces.iter().find(|p| p.unit == 0 && p.moved).unwrap();
        assert_eq!(bbox(&moved.rings), [7 * K, 0, 10 * K, 10 * K]);
        assert_eq!(of(1), vec![(1, false)]);
        let x = o.pieces.iter().find(|p| p.unit == 2).unwrap();
        assert_eq!((x.country, x.by), (1, By::Area));
    }

    /// A part narrower than the width moves nothing: the unit keeps its own vertices. A
    /// narrow rest moves with the wide part, and the unit moves whole, as drawn.
    #[test]
    fn narrow_parts_follow_the_wide_ones() {
        let (a, b) = (rect(0, 0, 10, 10), rect(10, 0, 20, 10));
        let view = [input("AAA", &a), input("BBB", &b)];
        // [9,10] in A is 0.91 wide.
        let b1 = rect(9, 0, 20, 10);
        let o = own(&view, &[Some("a"), Some("b")], &[input("b", &b1)], W);
        assert_eq!(o.pieces.len(), 1);
        assert_eq!(o.pieces[0].rings, b1);
        // [2,10] in A is wide, [10,10.5] in B narrow: all of it is A's.
        let b2 = vec![vec![
            (2 * K, 0),
            (10 * K + K / 2, 0),
            (10 * K + K / 2, 10 * K),
            (2 * K, 10 * K),
            (2 * K, 0),
        ]];
        let o = own(&view, &[Some("a"), Some("b")], &[input("b", &b2)], W);
        assert_eq!(o.pieces.len(), 1);
        assert_eq!((o.pieces[0].country, o.pieces[0].moved), (0, true));
        assert_eq!(o.pieces[0].rings, b2);
    }

    /// A large unit coded for a country it lies outside of moves whole to the one holding it;
    /// off every coast it is reported; a code nothing holds stays unowned.
    #[test]
    fn a_unit_far_from_its_code_moves_and_one_at_sea_is_reported() {
        // 1° squares: about 12,000 km², above the checked size.
        let big = |w: i32, s: i32| rect(w * 1000, s * 1000, w * 1000 + 1000, s * 1000 + 1000);
        let (a, b) = (big(0, 0), big(1, 0));
        let view = [input("AAA", &a), input("BBB", &b)];
        let sea = big(5, 5);
        let units = [input("a", &b), input("a", &sea), input("z", &sea)];
        let o = own(&view, &[Some("a"), Some("b")], &units, W);
        let p = o.pieces.iter().find(|p| p.unit == 0).unwrap();
        assert_eq!((p.country, p.moved), (1, true));
        assert_eq!(p.rings, b);
        assert_eq!(o.offshore.len(), 1);
        assert_eq!(o.offshore[0].0, 1);
        assert_eq!(o.unowned, vec![2]);
    }
}
