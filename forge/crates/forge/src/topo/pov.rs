//! First-level units cut to one point of view's countries (D-150).
//!
//! A world admin-1 source is drawn in its own point of view, the countries in another (Natural
//! Earth's admin 1 in the default view, the countries in Nepal's): a unit can lie in a country
//! other than its code's, or overhang its country. [`cut`] gives each unit the country that
//! holds most of it, cuts it to that country, and fills each country's remainder (what no unit
//! covers) with unnamed pieces, so every country is exactly the union of its pieces and the
//! topology stage dissolves it from them, sharing one arc set with its units.
//!
//! The two sources draw shared borders with slightly different vertices (a straight admin-0
//! segment passes within a grid unit of an admin-1 vertex), and an exact overlay turns that
//! into kilometre-long slivers narrower than a centimetre. A part narrower than [`MIN_WIDTH`]
//! (mean width `2 · area / perimeter`, about 1 m, the tolerance G-topo allows a dissolved
//! border) is noise: a unit is cut only by the wider parts of its overhang, so it keeps its own
//! lines everywhere else, and remainders that narrow are dropped. The overlay is
//! `i_overlay` on the 1e-7° grid with the `i64` engine, keeping every vertex (the `i32` engine
//! covers only ±2^30).

use std::collections::BTreeMap;

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay::{IntOverlayOptions, Overlay};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::core::solver::Solver;
use i_overlay::i_float::int::point::IntPoint;
use rayon::prelude::*;

use super::arcs::V;

pub(crate) mod own;

/// The code prefix of a filler piece: it has no place and links to none.
pub(crate) const FILLER: &str = "~";

/// A part narrower than this on average, in 1e-7° (about 1.1 m of latitude), is a difference
/// in how two sources drew one line, not a part of either.
pub(crate) const MIN_WIDTH: i128 = 100;

/// How far, metres, a country dissolved from its cut units may lie from the source's own line
/// (G-topo's parent check for this stage). The noise [`MIN_WIDTH`] leaves in place is narrow
/// on average but can bulge a few metres along a long border: 3.9 m one way and 5.2 m the other
/// on Natural Earth 5.1.2 (2026-11-10). A wrong cut or a lost piece is kilometres off, and the
/// sources are 1:10m drawings, so 10 m stops errors and passes the sources' disagreement.
pub(crate) const MAX_SHIFT_M: f64 = 10.0;

/// A polygon as overlay input: every ring a contour (even–odd), without the closing vertex.
type Shape = Vec<Vec<IntPoint<i64>>>;

/// A unit cut to its country.
#[derive(Debug)]
pub(crate) struct Piece {
    /// Its index in the units.
    pub(crate) unit: usize,
    /// The index of the country that holds most of it.
    pub(crate) country: usize,
    /// Its rings: the source's when it lies wholly in that country, else the overlay's.
    pub(crate) rings: Vec<Vec<V>>,
}

/// What [`cut`] made.
#[derive(Debug, Default)]
pub(crate) struct Cut {
    pub(crate) pieces: Vec<Piece>,
    /// The remainders: `(country, rings)`, one entry per connected part.
    pub(crate) fills: Vec<(usize, Vec<Vec<V>>)>,
    /// Units whose country is not the one their code names.
    pub(crate) moved: Vec<usize>,
    /// Units cut (with a part at least [`MIN_WIDTH`] wide outside their country).
    pub(crate) clipped: usize,
    /// Units the point of view does not recognise, dropped.
    pub(crate) unrecognized: usize,
    /// Units in no country at all (off every coast), dropped.
    pub(crate) outside: usize,
    /// Units of countries whose units come from elsewhere (`keep` false), dropped.
    pub(crate) skipped: usize,
}

/// One country or unit for the cut: its code (the country's own, or the country the unit's
/// code names) and its rings on the 1e-7° grid as `(lon, lat)`.
pub(crate) struct Input<'a> {
    pub(crate) code: &'a str,
    pub(crate) rings: &'a [Vec<V>],
}

fn shape(rings: &[Vec<V>]) -> Shape {
    rings
        .iter()
        .map(|r| {
            let n = if r.len() > 1 && r.first() == r.last() {
                r.len() - 1
            } else {
                r.len()
            };
            r.iter()
                .take(n)
                .map(|&(x, y)| IntPoint::new(i64::from(x), i64::from(y)))
                .collect()
        })
        .filter(|c: &Vec<IntPoint<i64>>| c.len() >= 3)
        .collect()
}

fn overlay(subj: &Shape, clip: &Shape, rule: OverlayRule) -> Vec<Shape> {
    Overlay::<i64>::from_subj_and_clip_custom(
        subj,
        clip,
        IntOverlayOptions::keep_all_points(),
        Solver::default(),
    )
    .overlay(rule, FillRule::EvenOdd)
}

/// Twice the area enclosed, holes subtracted (outer contours counter-clockwise, holes
/// clockwise, as the overlay writes them).
fn area2(shapes: &[Shape]) -> i128 {
    shapes
        .iter()
        .flatten()
        .map(|c| {
            c.iter()
                .zip(c.iter().cycle().skip(1))
                .map(|(a, b)| i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y))
                .sum::<i128>()
        })
        .sum()
}

/// The length of every contour of `shapes`, in grid units.
fn perimeter(shapes: &[Shape]) -> f64 {
    shapes
        .iter()
        .flatten()
        .flat_map(|c| c.iter().zip(c.iter().cycle().skip(1)))
        .map(|(a, b)| {
            #[allow(clippy::cast_precision_loss, reason = "differences below 2^33")]
            let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
            dx.hypot(dy)
        })
        .sum()
}

/// Whether `s` (one outer contour and its holes) is narrower than `width` grid units on
/// average (mean width `2 · area / perimeter`).
fn narrower(s: &Shape, width: i128) -> bool {
    let area = area2(std::slice::from_ref(s)).abs();
    let perimeter = perimeter(std::slice::from_ref(s));
    // Twice the area over the perimeter, compared without dividing: 2A/P < w ⇔ 2A < w·P.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "a perimeter far below 2^60 grid units"
    )]
    let wp = (width as f64 * perimeter) as i128;
    area < wp
}

/// The parts of `shapes` at least [`MIN_WIDTH`] wide.
fn wide(shapes: Vec<Shape>) -> Vec<Shape> {
    shapes
        .into_iter()
        .filter(|s| !narrower(s, MIN_WIDTH))
        .collect()
}

/// `subject` without `clips`, in connected parts at least `min_width` grid units wide on
/// average; `None` when the clips take nothing from it, so the caller keeps its own vertices
/// (D-155). Clips may overlap each other: every operand is first normalised (each read
/// even–odd, outer contours counter-clockwise and holes clockwise), and the difference is
/// taken non-zero, so the clips act as their union.
pub(crate) fn minus(
    subject: &[Vec<V>],
    clips: &[&[Vec<V>]],
    min_width: i128,
) -> Option<Vec<Vec<Vec<V>>>> {
    let normal = |rings: &[Vec<V>]| -> Shape {
        overlay(&shape(rings), &Shape::new(), OverlayRule::Subject)
            .into_iter()
            .flatten()
            .collect()
    };
    let whole = normal(subject);
    let clip: Shape = clips.iter().flat_map(|c| normal(c)).collect();
    let parts = Overlay::<i64>::from_subj_and_clip_custom(
        &whole,
        &clip,
        IntOverlayOptions::keep_all_points(),
        Solver::default(),
    )
    .overlay(OverlayRule::Difference, FillRule::NonZero);
    if area2(&parts) == area2(std::slice::from_ref(&whole)) {
        return None;
    }
    Some(
        parts
            .into_iter()
            .filter(|s| !narrower(s, min_width))
            .map(|s| rings_of(std::slice::from_ref(&s)))
            .collect(),
    )
}

/// `rings` read even–odd as normalised parts: outer contours counter-clockwise, holes
/// clockwise, a zero-width bridge or a ring touching itself split (D-155).
pub(crate) fn normal(rings: &[Vec<V>]) -> Vec<Vec<Vec<V>>> {
    overlay(&shape(rings), &Shape::new(), OverlayRule::Subject)
        .iter()
        .map(|s| rings_of(std::slice::from_ref(s)))
        .collect()
}

/// [`normal`]'s parts at least `min_width` grid units wide on average.
pub(crate) fn normal_wide(rings: &[Vec<V>], min_width: i128) -> Vec<Vec<Vec<V>>> {
    overlay(&shape(rings), &Shape::new(), OverlayRule::Subject)
        .iter()
        .filter(|s| !narrower(s, min_width))
        .map(|s| rings_of(std::slice::from_ref(s)))
        .collect()
}

/// Whether `sets` overlap anywhere by more than rounding: the area of their union is less than
/// the sum of their areas (each read even–odd, the union non-zero). One overlay.
pub(crate) fn overlapping(sets: &[&[Vec<V>]]) -> bool {
    let normal: Vec<Shape> = sets
        .iter()
        .map(|r| {
            overlay(&shape(r), &Shape::new(), OverlayRule::Subject)
                .into_iter()
                .flatten()
                .collect()
        })
        .collect();
    let sum: i128 = normal.iter().map(|s| area2(std::slice::from_ref(s))).sum();
    let all: Shape = normal.into_iter().flatten().collect();
    let union = Overlay::<i64>::from_subj_and_clip_custom(
        &all,
        &Shape::new(),
        IntOverlayOptions::keep_all_points(),
        Solver::default(),
    )
    .overlay(OverlayRule::Subject, FillRule::NonZero);
    area2(&union) < sum
}

/// The parts of `a` and `b` in common at least `min_width` grid units wide on average, as one
/// set of rings (empty when they only touch).
pub(crate) fn common(a: &[Vec<V>], b: &[Vec<V>], min_width: i128) -> Vec<Vec<V>> {
    let parts: Vec<Shape> = overlay(&shape(a), &shape(b), OverlayRule::Intersect)
        .into_iter()
        .filter(|s| !narrower(s, min_width))
        .collect();
    rings_of(&parts)
}

/// The neighbour sharing the most border with `gap` (more than noise, [`MIN_SHARED`]); ties
/// to the lower index.
pub(crate) fn best_neighbour<'a>(
    gap: &[Vec<V>],
    near: impl Iterator<Item = (usize, &'a [Vec<V>])>,
) -> Option<usize> {
    let g = vec![shape(gap)];
    let pg = perimeter(&g);
    near.map(|(i, n)| {
        let s = vec![shape(n)];
        let joined = union(vec![s[0].clone(), g[0].clone()]);
        // Twice the border they share: what their union loses of their perimeters.
        (perimeter(&s) + pg - perimeter(&joined), i)
    })
    .filter(|b| b.0 > MIN_SHARED)
    .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
    .map(|b| b.1)
}

/// The union of sets of rings (each read even–odd) that are disjoint or touch, as rings
/// (outer counter-clockwise, holes clockwise): a country dissolved from its pieces.
pub(crate) fn dissolve(sets: &[&[Vec<V>]]) -> Vec<Vec<V>> {
    rings_of(&union(
        sets.iter()
            .map(|r| {
                overlay(&shape(r), &Shape::new(), OverlayRule::Subject)
                    .into_iter()
                    .flatten()
                    .collect()
            })
            .collect(),
    ))
}

/// The union of normalised shapes (non-zero).
fn union(shapes: Vec<Shape>) -> Vec<Shape> {
    let all: Shape = shapes.into_iter().flatten().collect();
    if all.is_empty() {
        return Vec::new();
    }
    Overlay::<i64>::from_subj_and_clip_custom(
        &all,
        &Shape::new(),
        IntOverlayOptions::keep_all_points(),
        Solver::default(),
    )
    .overlay(OverlayRule::Subject, FillRule::NonZero)
}

/// Back to rings of `(lon, lat)`; the overlay's points are on the input grid.
fn rings_of(shapes: &[Shape]) -> Vec<Vec<V>> {
    shapes
        .iter()
        .flatten()
        .map(|c| {
            c.iter()
                .filter_map(|p| Some((i32::try_from(p.x).ok()?, i32::try_from(p.y).ok()?)))
                .collect()
        })
        .collect()
}

/// West, south, east, north of the rings.
pub(crate) fn bbox(rings: &[Vec<V>]) -> [i32; 4] {
    rings
        .iter()
        .flatten()
        .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, &(x, y)| {
            [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]
        })
}

/// Whether two boxes meet.
pub(crate) const fn meets(a: &[i32; 4], b: &[i32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Where one unit goes.
enum Fate {
    /// A piece, whether it was cut, and whether its country is not its code's.
    Piece(Piece, bool, bool),
    Unrecognized,
    Outside,
}

/// The countries' shapes, boxes and contour boxes, and each country's index by code.
struct Countries<'a> {
    shapes: Vec<Shape>,
    boxes: Vec<[i32; 4]>,
    /// Per country, the box of each of its contours, in the order of its shape.
    contours: Vec<Vec<[i32; 4]>>,
    by_code: BTreeMap<&'a str, usize>,
    /// Countries whose outline no other piece may overlap at all (a pack country's, D-154).
    exact: Vec<usize>,
}

impl<'a> Countries<'a> {
    fn new(countries: &[Input<'a>], exact: &[usize]) -> Self {
        let shapes: Vec<Shape> = countries.par_iter().map(|c| shape(c.rings)).collect();
        Self {
            boxes: countries.iter().map(|c| bbox(c.rings)).collect(),
            contours: shapes
                .iter()
                .map(|s| s.iter().map(|k| contour_box(k)).collect())
                .collect(),
            shapes,
            by_code: countries
                .iter()
                .enumerate()
                .map(|(i, c)| (c.code, i))
                .collect(),
            exact: exact.to_vec(),
        }
    }

    /// Country `k` as far as it can matter inside `bounds`: the contours whose box meets it.
    /// Under even–odd a contour encloses only points inside its own box, so the others change
    /// nothing there. This bounds each overlay by the unit's neighbourhood, not by the whole
    /// country (Russia, Canada, France with its overseas parts).
    fn near(&self, k: usize, bounds: &[i32; 4]) -> Option<Shape> {
        let (shape, boxes) = (self.shapes.get(k)?, self.contours.get(k)?);
        Some(
            shape
                .iter()
                .zip(boxes)
                .filter(|(_, b)| meets(b, bounds))
                .map(|(c, _)| c.clone())
                .collect(),
        )
    }
}

fn contour_box(c: &[IntPoint<i64>]) -> [i32; 4] {
    let clamp = |v: i64| i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX });
    c.iter()
        .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, p| {
            let (x, y) = (clamp(p.x), clamp(p.y));
            [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]
        })
}

/// Unit `unit` in the country that holds most of it (its own code's when that holds all of it
/// but noise; ties to the lower index), minus the wide parts of its overhang: as drawn when
/// there are none, so its vertices stay the source's. Then minus every exact country's outline,
/// whatever the width ([`Countries::exact`]).
fn place(countries: &Countries<'_>, unit: usize, u: &Input<'_>) -> Fate {
    match place_loose(countries, unit, u) {
        Fate::Piece(p, was_cut, moved) => match off_exact(countries, &p) {
            Some(rings) if rings.is_empty() => Fate::Outside,
            Some(rings) => Fate::Piece(Piece { rings, ..p }, true, moved),
            None => Fate::Piece(p, was_cut, moved),
        },
        f => f,
    }
}

/// Piece `p` minus the exact countries other than its own that overlap it; `None` when none do.
fn off_exact(countries: &Countries<'_>, p: &Piece) -> Option<Vec<Vec<V>>> {
    let bounds = bbox(&p.rings);
    let clip: Shape = countries
        .exact
        .iter()
        .filter(|&&k| k != p.country)
        .filter(|&&k| countries.boxes.get(k).is_some_and(|b| meets(b, &bounds)))
        .filter_map(|&k| countries.near(k, &bounds))
        .flatten()
        .collect();
    if clip.is_empty() {
        return None;
    }
    let drawn = shape(&p.rings);
    if area2(&overlay(&drawn, &clip, OverlayRule::Intersect)) == 0 {
        return None;
    }
    Some(rings_of(&wide(overlay(
        &drawn,
        &clip,
        OverlayRule::Difference,
    ))))
}

fn place_loose(countries: &Countries<'_>, unit: usize, u: &Input<'_>) -> Fate {
    let drawn = shape(u.rings);
    let bounds = bbox(u.rings);
    let own = countries.by_code.get(u.code).copied();
    let overhang = |k: usize| {
        countries
            .near(k, &bounds)
            .map(|cs| wide(overlay(&drawn, &cs, OverlayRule::Difference)))
    };
    let as_drawn = |k: usize| Piece {
        unit,
        country: k,
        rings: u.rings.to_vec(),
    };
    if let Some(o) = own
        && countries.boxes.get(o).is_some_and(|cb| meets(&bounds, cb))
        && overhang(o).is_some_and(|w| w.is_empty())
    {
        return Fate::Piece(as_drawn(o), false, false);
    }
    let best = countries
        .boxes
        .iter()
        .enumerate()
        .filter(|(_, cb)| meets(&bounds, cb))
        .filter_map(|(k, _)| {
            let inter = wide(overlay(
                &drawn,
                &countries.near(k, &bounds)?,
                OverlayRule::Intersect,
            ));
            let area = area2(&inter);
            (area > 0).then_some((area, k))
        })
        .max_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)));
    let Some((_, k)) = best else {
        return Fate::Outside;
    };
    let over = overhang(k).unwrap_or_default();
    if over.is_empty() {
        return Fate::Piece(as_drawn(k), false, own != Some(k));
    }
    let cut_off: Shape = over.into_iter().flatten().collect();
    let kept = wide(overlay(&drawn, &cut_off, OverlayRule::Difference));
    let piece = Piece {
        unit,
        country: k,
        rings: rings_of(&kept),
    };
    Fate::Piece(piece, true, own != Some(k))
}

/// Each country's remainder: the country minus its pieces, one entry per connected part (the
/// whole country when it has none).
fn remainders(c: &Countries<'_>, pieces: &[Piece]) -> Vec<(usize, Vec<Vec<V>>)> {
    let mut of_country: Vec<Vec<usize>> = vec![Vec::new(); c.shapes.len()];
    for (k, p) in pieces.iter().enumerate() {
        if let Some(v) = of_country.get_mut(p.country) {
            v.push(k);
        }
    }
    let fills: Vec<Vec<(usize, Vec<Vec<V>>)>> = of_country
        .par_iter()
        .enumerate()
        .map(|(k, members)| {
            let Some(cs) = c.shapes.get(k) else {
                return Vec::new();
            };
            let rest = if members.is_empty() {
                vec![cs.clone()]
            } else {
                // A source's units tile, so even–odd on all their rings is their union; an
                // overlap would leave a country unequal to its dissolve, which G-topo stops.
                let clip: Shape = members
                    .iter()
                    .filter_map(|&m| pieces.get(m))
                    .flat_map(|p| shape(&p.rings))
                    .collect();
                wide(overlay(cs, &clip, OverlayRule::Difference))
            };
            rest.iter()
                .map(|s| (k, rings_of(std::slice::from_ref(s))))
                .collect()
        })
        .collect();
    fills.into_iter().flatten().collect()
}

/// Cuts `units` to `countries`. `unrecognized` marks units the point of view does not
/// recognise; `keep` says whether a country takes its units from this source (a country with
/// a pack takes them from its pack). Deterministic; parallel over units, then countries.
pub(crate) fn cut(
    countries: &[Input<'_>],
    units: &[Input<'_>],
    unrecognized: &[bool],
    keep: impl Fn(usize) -> bool + Sync,
    exact: &[usize],
) -> Cut {
    let c = Countries::new(countries, exact);
    let fates: Vec<Fate> = units
        .par_iter()
        .enumerate()
        .map(|(i, u)| {
            if unrecognized.get(i).copied().unwrap_or(false) {
                Fate::Unrecognized
            } else {
                place(&c, i, u)
            }
        })
        .collect();
    let mut out = Cut::default();
    for f in fates {
        match f {
            Fate::Piece(p, was_cut, moved) => {
                if !keep(p.country) {
                    out.skipped += 1;
                    continue;
                }
                out.clipped += usize::from(was_cut);
                if moved {
                    out.moved.push(p.unit);
                }
                out.pieces.push(p);
            }
            Fate::Unrecognized => out.unrecognized += 1,
            Fate::Outside => out.outside += 1,
        }
    }
    out.fills = remainders(&c, &out.pieces);
    yield_exact(&c, &mut out);
    out
}

/// An exact country's fillers give way to every other piece and filler near them: the overlay
/// rounds a crossing to the grid, so a neighbour cut by the exact outline can still reach a
/// grid unit (about 1 cm) into it. Cut last, the exact country takes its neighbours' vertices
/// where they meet, so no two pieces overlap, and its line moves at most that unit.
fn yield_exact(c: &Countries<'_>, out: &mut Cut) {
    let others: Vec<(usize, [i32; 4], Shape)> = out
        .pieces
        .iter()
        .map(|p| (p.country, &p.rings))
        .chain(out.fills.iter().map(|(k, r)| (*k, r)))
        .map(|(k, r)| (k, bbox(r), shape(r)))
        .collect();
    let mut fills = Vec::with_capacity(out.fills.len());
    for (k, rings) in std::mem::take(&mut out.fills) {
        if !c.exact.contains(&k) {
            fills.push((k, rings));
            continue;
        }
        let bounds = bbox(&rings);
        let clip: Shape = others
            .iter()
            .filter(|(o, b, _)| *o != k && meets(b, &bounds))
            .flat_map(|(_, _, s)| s.iter().cloned())
            .collect();
        if clip.is_empty() {
            fills.push((k, rings));
            continue;
        }
        let rest = wide(overlay(&shape(&rings), &clip, OverlayRule::Difference));
        fills.extend(rest.iter().map(|s| (k, rings_of(std::slice::from_ref(s)))));
    }
    out.fills = fills;
}

/// A country's index and the outline its pack draws.
pub(crate) type Outline = (usize, Vec<Vec<V>>);

/// Twice the border, in grid units, a neighbour must share with a gap to take it: more than
/// noise ([`MIN_WIDTH`] each way).
const MIN_SHARED: f64 = 200.0;

/// What [`redraw`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Redrawn {
    /// Neighbours cut by a pack country's outline.
    pub(crate) cut: usize,
    /// Pieces of a pack country's source outline outside its own outline, each given to the
    /// neighbour sharing the most border with it.
    pub(crate) given: usize,
    /// Such pieces no neighbour touches, left to no country.
    pub(crate) left: usize,
}

/// Redraws the countries around each pack country's own outline (D-154): `outlines` holds a
/// country index and the outline its pack draws (the vertices its baseline slice ships). The
/// pack country takes that outline; every other country is cut by it, keeping its parts at
/// least [`MIN_WIDTH`] wide; and each part of the pack country's source outline that its own
/// outline leaves out goes to the neighbour sharing the most border with it. Pack countries
/// never cut each other: each keeps its pack's line.
pub(crate) fn redraw(countries: &mut [Vec<Vec<V>>], outlines: &[Outline]) -> Redrawn {
    let mut r = Redrawn::default();
    let packs: Vec<usize> = outlines.iter().map(|o| o.0).collect();
    for (p, outline) in outlines {
        let new = shape(outline);
        let Some(source) = countries.get(*p) else {
            continue;
        };
        let old = shape(source);
        let area = [bbox(outline), bbox(source)];
        let near: Vec<usize> = (0..countries.len())
            .filter(|i| !packs.contains(i))
            .filter(|&i| {
                countries
                    .get(i)
                    .is_some_and(|c| area.iter().any(|b| meets(&bbox(c), b)))
            })
            .collect();
        for &i in &near {
            let Some(c) = countries.get_mut(i) else {
                continue;
            };
            let s = shape(c);
            if area2(&overlay(&s, &new, OverlayRule::Intersect)) == 0 {
                continue;
            }
            *c = rings_of(&wide(overlay(&s, &new, OverlayRule::Difference)));
            r.cut += 1;
        }
        for gap in wide(overlay(&old, &new, OverlayRule::Difference)) {
            let gap = vec![gap];
            let gap = rings_of(&gap);
            let best = best_neighbour(
                &gap,
                near.iter()
                    .filter_map(|&i| Some((i, countries.get(i)?.as_slice()))),
            );
            match best {
                Some(i) => {
                    if let Some(c) = countries.get_mut(i) {
                        *c = dissolve(&[c, &gap]);
                    }
                    r.given += 1;
                }
                None => r.left += 1,
            }
        }
        if let Some(c) = countries.get_mut(*p) {
            c.clone_from(outline);
        }
    }
    r
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::many_single_char_names,
        reason = "test code; a, b name the countries and a1, x, u the units"
    )]

    use super::*;

    /// One model unit, 10,000 grid units (about 110 m): far wider than [`MIN_WIDTH`].
    const K: i32 = 10_000;

    fn rect(w: i32, s: i32, e: i32, n: i32) -> Vec<Vec<V>> {
        let (w, s, e, n) = (w * K, s * K, e * K, n * K);
        vec![vec![(w, s), (e, s), (e, n), (w, n), (w, s)]]
    }

    /// Area in model units² (twice the grid area over K², halved).
    fn area(rings: &[Vec<V>]) -> i128 {
        area2(&[shape(rings)]).abs() / i128::from(K) / i128::from(K) / 2
    }

    /// Country A is [0,10]², B is [10,20]×[0,10] (model units). Unit a1 is A's west half, drawn under A; unit
    /// a2 is drawn under A but reaches 2 into B (a view moved the border); unit b1 is drawn
    /// under code C (no such country) but lies in B; unit x is in no country; unit u is
    /// unrecognised.
    #[test]
    fn units_take_the_country_holding_most_of_them_and_remainders_fill() {
        let (a, b) = (rect(0, 0, 10, 10), rect(10, 0, 20, 10));
        let countries = [
            Input {
                code: "A",
                rings: &a,
            },
            Input {
                code: "B",
                rings: &b,
            },
        ];
        let (a1, a2, b1, x, u) = (
            rect(0, 0, 5, 10),
            rect(5, 0, 12, 10),
            rect(14, 0, 20, 5),
            rect(30, 30, 31, 31),
            rect(16, 6, 18, 8),
        );
        let units = [
            Input {
                code: "A",
                rings: &a1,
            },
            Input {
                code: "A",
                rings: &a2,
            },
            Input {
                code: "C",
                rings: &b1,
            },
            Input {
                code: "A",
                rings: &x,
            },
            Input {
                code: "B",
                rings: &u,
            },
        ];
        let c = cut(
            &countries,
            &units,
            &[false, false, false, false, true],
            |_| true,
            &[],
        );
        assert_eq!((c.unrecognized, c.outside, c.skipped), (1, 1, 0));
        assert_eq!(c.moved, [2]);
        // a2 overhangs B; b1 is drawn under C but lies wholly in B, so it moves uncut.
        assert_eq!(c.clipped, 1, "only a2 is cut");
        let piece = |unit: usize| c.pieces.iter().find(|p| p.unit == unit).unwrap();
        // a1 lies in A: kept exactly as drawn.
        assert_eq!((piece(0).country, &piece(0).rings), (0, &a1));
        // a2 is cut to A: [5,10]×[0,10].
        assert_eq!((piece(1).country, area(&piece(1).rings)), (0, 50));
        assert_eq!(piece(2).country, 1);
        // Remainders: A is covered; B's is B minus b1 ([10,20]×[0,10] minus [14,20]×[0,5]).
        assert!(c.fills.iter().all(|f| f.0 == 1));
        let rest: i128 = c.fills.iter().map(|f| area(&f.1)).sum();
        assert_eq!(rest, 100 - 30);
        // Every country is exactly its pieces and fills.
        for (k, country) in [&a, &b].iter().enumerate() {
            let total: i128 = c
                .pieces
                .iter()
                .filter(|p| p.country == k)
                .map(|p| area(&p.rings))
                .chain(c.fills.iter().filter(|f| f.0 == k).map(|f| area(&f.1)))
                .sum();
            assert_eq!(total, area(country), "country {k}");
        }
        // `keep` false: B's units dropped and B is one remainder.
        let c = cut(&countries, &units, &[false; 5], |k| k == 0, &[]);
        assert_eq!(c.skipped, 2);
        assert_eq!(c.fills.iter().filter(|f| f.0 == 1).count(), 1);
    }

    /// Noise: a unit that pokes 1 grid unit (about 1 cm) over its country's border stays as
    /// drawn, and a remainder 1 grid unit wide is dropped, so the two sources' different
    /// vertices along one line make no slivers.
    #[test]
    fn parts_narrower_than_a_metre_are_noise() {
        let a = rect(0, 0, 10, 10);
        let countries = [Input {
            code: "A",
            rings: &a,
        }];
        // Over the east border by one grid unit; short of the north border by one.
        let (e, n) = (10 * K + 1, 10 * K - 1);
        let u = vec![vec![(0, 0), (e, 0), (e, n), (0, n), (0, 0)]];
        let units = [Input {
            code: "A",
            rings: &u,
        }];
        let c = cut(&countries, &units, &[false], |_| true, &[]);
        assert_eq!((c.clipped, c.pieces.len()), (0, 1));
        assert_eq!(c.pieces[0].rings, u, "kept as drawn");
        assert!(
            c.fills.is_empty(),
            "the 1-unit strip along the north is noise"
        );
        // A real overhang (a tenth of the country) is still cut, by its wide part only.
        let v = rect(0, 0, 11, 10);
        let units = [Input {
            code: "A",
            rings: &v,
        }];
        let c = cut(&countries, &units, &[false], |_| true, &[]);
        assert_eq!((c.clipped, area(&c.pieces[0].rings)), (1, 100));
    }

    /// A pack country's own outline (D-154): A is [0,10]², B east of it, C north of it. A's
    /// pack draws it as [0,12]×[0,9]: B loses the overlap, and the strip [0,10]×[9,10] that A's
    /// pack leaves out goes to C, which shares 10 of its border against B's 1. The total area
    /// stays.
    #[test]
    fn a_pack_outline_cuts_its_neighbours_and_gives_away_its_gaps() {
        let mut countries = vec![rect(0, 0, 10, 10), rect(10, 0, 20, 10), rect(0, 10, 10, 20)];
        let own = rect(0, 0, 12, 9);
        let r = redraw(&mut countries, &[(0, own.clone())]);
        assert_eq!(
            r,
            Redrawn {
                cut: 1,
                given: 1,
                left: 0
            }
        );
        assert_eq!(countries[0], own);
        let areas: Vec<i128> = countries.iter().map(|c| area(c)).collect();
        assert_eq!(areas, [108, 82, 110]);
        // B and C no longer overlap A.
        for c in &countries[1..] {
            assert_eq!(
                area2(&overlay(&shape(c), &shape(&own), OverlayRule::Intersect)),
                0
            );
        }
        // A second pack country is never cut by the first.
        let mut two = vec![rect(0, 0, 10, 10), rect(10, 0, 20, 10)];
        let r = redraw(
            &mut two,
            &[(0, rect(0, 0, 12, 10)), (1, rect(10, 0, 20, 10))],
        );
        assert_eq!(r.cut, 0);
        assert_eq!(two[1], rect(10, 0, 20, 10));
    }

    /// An exact country (a pack's outline, D-154): a neighbour's unit that overhangs it by less
    /// than [`MIN_WIDTH`] keeps that sliver as noise against an ordinary country, but loses it
    /// against an exact one, so no piece overlaps the pack's line at all.
    #[test]
    fn no_piece_overlaps_an_exact_country() {
        let (a, b) = (rect(0, 0, 10, 10), rect(10, 0, 20, 10));
        let countries = [
            Input {
                code: "A",
                rings: &a,
            },
            Input {
                code: "B",
                rings: &b,
            },
        ];
        // B's unit reaches 50 grid units (about 55 cm) into A.
        let thin = vec![vec![
            (10 * K - 50, 0),
            (20 * K, 0),
            (20 * K, 10 * K),
            (10 * K - 50, 10 * K),
            (10 * K - 50, 0),
        ]];
        let units = [Input {
            code: "B",
            rings: &thin,
        }];
        let inside_a = |c: &Cut| {
            let p = c.pieces.first().unwrap();
            area2(&overlay(
                &shape(&p.rings),
                &shape(&a),
                OverlayRule::Intersect,
            ))
        };
        let loose = cut(&countries, &units, &[false], |_| true, &[]);
        assert!(inside_a(&loose) > 0, "noise against an ordinary country");
        let exact = cut(&countries, &units, &[false], |_| true, &[0]);
        assert_eq!(inside_a(&exact), 0);
        assert_eq!(exact.clipped, 1);
    }
}
