//! Modelled population of admin units (D-123): a population grid summed over each unit.
//!
//! Each grid cell with people counts once, in the deepest unit holding its centre; a unit's
//! population is the sum over it and its descendants, so the levels add up exactly. A stated
//! count (a linked record's, often a census) stays when it is within [`PLAUSIBLE`] of the
//! modelled one; otherwise the modelled one replaces it: a linked record can carry another
//! unit's count (Kathmandu District's 2,041,587 on the metropolitan city). Every unit's two
//! values are written for review. The grid is read one chunk at a time ([`crate::raster`]), so
//! memory is one chunk plus one number per unit at any extent.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use teigeodata_core::PlaceKind;

use super::Place;
use super::units::Level;
use crate::raster;

/// What summing a grid over the units gave.
#[derive(Debug, Default)]
pub(super) struct Modelled {
    /// People in each unit (every level), by key.
    pub(super) by_key: HashMap<String, f64>,
    /// People in the grid's cells inside the box, and outside every finest unit.
    pub(super) total: f64,
    pub(super) outside: f64,
    pub(super) cells: u64,
}

/// A stated population stays when the modelled one is within this factor of it.
pub(super) const PLAUSIBLE: f64 = 1.5;

/// The modelled populations as built: the summary lines and each unit's values.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    pub(super) report: String,
    rows: Vec<Row>,
}

/// One unit: key, level, name, the stated and modelled populations, and the one kept.
#[derive(Debug)]
struct Row {
    key: String,
    level: u8,
    name: String,
    stated: Option<i64>,
    modelled: i64,
    kept: Option<i64>,
}

impl Outcome {
    /// The keys of the units whose population is still the modelled one (D-351): the grid's
    /// value replaced or filled the stated one, and nothing after (a correction) changed it.
    pub(super) fn modelled(&self, places: &[Place]) -> std::collections::BTreeSet<String> {
        let now: HashMap<&str, Option<i64>> = places
            .iter()
            .map(|p| (p.key.as_str(), p.population))
            .collect();
        self.rows
            .iter()
            .filter(|r| r.kept != r.stated && r.kept == Some(r.modelled))
            .filter(|r| now.get(r.key.as_str()).copied().flatten() == r.kept)
            .map(|r| r.key.clone())
            .collect()
    }

    /// Writes `population-<build>.tsv` beside the pack (nothing without a grid).
    pub(super) fn write(&self, dir: &Path, build: &str) -> Result<(), String> {
        if self.rows.is_empty() {
            return Ok(());
        }
        let mut t = String::from(
            "# Admin units' population (D-123): stated (a linked record's), modelled (the grid), kept.\nkey\tlevel\tname\tstated\tmodelled\tkept\n",
        );
        let opt = |v: Option<i64>| v.map_or(String::new(), |v| v.to_string());
        for r in &self.rows {
            let _ = writeln!(
                t,
                "{}\t{}\t{}\t{}\t{}\t{}",
                r.key,
                r.level,
                r.name,
                opt(r.stated),
                r.modelled,
                opt(r.kept)
            );
        }
        let path = dir.join(format!("population-{build}.tsv"));
        std::fs::write(&path, t).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Models the population of the units of `levels` (inside `bbox`, 1e-7°) from the grids
/// `paths` and gives it to the current admin units of `places` ([`fill`]).
pub(super) fn run(
    paths: &[PathBuf],
    levels: &[Level],
    bbox: (i32, i32, i32, i32),
    places: &mut [Place],
) -> Result<Outcome, String> {
    if paths.is_empty() {
        return Ok(Outcome::default());
    }
    let started = std::time::Instant::now();
    let m = model(paths, levels, places, bbox)?;
    let mut report = compare(places, &m);
    let rows = fill(places, &m);
    let (took, replaced) = rows
        .iter()
        .fold((0, 0), |(t, r), row| match (row.stated, row.kept) {
            (None, Some(_)) => (t + 1, r),
            (Some(s), Some(k)) if s != k => (t, r + 1),
            _ => (t, r),
        });
    let _ = writeln!(
        report,
        "{took} units took a modelled population; {replaced} stated ones off by more than {PLAUSIBLE}× were replaced ({:.1?}).",
        started.elapsed()
    );
    Ok(Outcome { report, rows })
}

/// Sums the grids `paths` over `levels` inside `bbox` (1e-7°, as the units' coverage): each
/// cell into the deepest unit that holds it, then up each unit's parent chain in `places`. A
/// level with fillers (a derived set's wards, D-363) leaves land to the level above: summed
/// over the finest level alone, a local level with no ward (a national park) took no modelled
/// count and kept a linked record's wrong one, and a filler's people were lost to its parent.
pub(super) fn model(
    paths: &[PathBuf],
    levels: &[Level],
    places: &[Place],
    (x0, y0, x1, y1): (i32, i32, i32, i32),
) -> Result<Modelled, String> {
    let mut m = Modelled::default();
    if levels.is_empty() {
        return Ok(m);
    }
    let mut sums: Vec<Vec<f64>> = levels
        .iter()
        .map(|l| vec![0.0_f64; l.polys.len()])
        .collect();
    let e7 = |v: i32| f64::from(v) / 1e7;
    for path in paths {
        // Consecutive cells mostly fall in the same unit: try the last one first.
        let mut last: Option<(usize, usize)> = None;
        raster::for_each_positive(path, (e7(x0), e7(y0), e7(x1), e7(y1)), |lon, lat, v| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "degrees within ±180 on the 1e-7° grid fit i32"
            )]
            let p = ((lon * 1e7).round() as i32, (lat * 1e7).round() as i32);
            m.cells += 1;
            m.total += v;
            let hit = deepest_holding(levels, p, last);
            match hit.and_then(|(l, i)| sums.get_mut(l)?.get_mut(i).map(|s| ((l, i), s))) {
                Some((li, s)) => {
                    *s += v;
                    last = Some(li);
                }
                None => m.outside += v,
            }
        })?;
    }
    up_chains(levels, &sums, places, &mut m.by_key);
    Ok(m)
}

/// The unit holding `p` at the deepest level that has one, as (level, polygon); `last` is the
/// previous cell's, tried first when it is of the deepest level.
fn deepest_holding(
    levels: &[Level],
    p: (i32, i32),
    last: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let holds = |(l, i): (usize, usize)| {
        levels
            .get(l)
            .and_then(|lv| lv.polys.get(i))
            .is_some_and(|q| q.contains(p.0, p.1))
    };
    last.filter(|&li| li.0 + 1 == levels.len() && holds(li))
        .or_else(|| {
            levels.iter().enumerate().rev().find_map(|(l, lv)| {
                lv.index
                    .candidates(p.0, p.1)
                    .map(|i| (l, i))
                    .find(|&li| holds(li))
            })
        })
}

/// Adds each unit's own sum (`sums`, per level and polygon) to it and every unit up its
/// parent chain in `places`.
fn up_chains(
    levels: &[Level],
    sums: &[Vec<f64>],
    places: &[Place],
    by_key: &mut HashMap<String, f64>,
) {
    let parent: HashMap<&str, Option<&str>> = places
        .iter()
        .map(|p| (p.key.as_str(), p.parent.as_deref()))
        .collect();
    for (level, sums) in levels.iter().zip(sums) {
        for (key, &s) in level.keys.iter().zip(sums) {
            if s == 0.0 {
                continue;
            }
            let mut at = Some(key.as_str());
            // A parent chain is at most as long as the levels; the bound guards a cycle.
            for _ in 0..=levels.len() {
                let Some(k) = at else { break };
                *by_key.entry(k.to_string()).or_default() += s;
                at = parent.get(k).copied().flatten();
            }
        }
    }
}

/// Gives every current admin unit its population: the stated one when the modelled one is
/// within [`PLAUSIBLE`] of it, else the modelled one (rounded; none below half a person).
fn fill(places: &mut [Place], m: &Modelled) -> Vec<Row> {
    let mut rows = Vec::new();
    for p in places.iter_mut().filter(|p| p.kind == PlaceKind::Admin) {
        let Some(&v) = m.by_key.get(&p.key) else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation, reason = "a count of people")]
        let modelled = v.round() as i64;
        let stated = p.population;
        #[allow(clippy::cast_precision_loss, reason = "a count of people")]
        let plausible =
            |s: i64| s > 0 && (v / s as f64) <= PLAUSIBLE && (s as f64 / v) <= PLAUSIBLE;
        p.population = match stated {
            Some(s) if plausible(s) => Some(s),
            _ if modelled > 0 => Some(modelled),
            _ => stated,
        };
        rows.push(Row {
            key: p.key.clone(),
            level: p.admin_level,
            name: p.names.first().map_or_else(String::new, |n| n.text.clone()),
            stated,
            modelled,
            kept: p.population,
        });
    }
    rows
}

/// The modelled population against the stated one, per level: units with both, the median and
/// 90th-percentile absolute error in percent, and the median ratio (modelled / stated).
pub(super) fn compare(places: &[Place], m: &Modelled) -> String {
    let mut by_level: BTreeMap<u8, Vec<(f64, f64)>> = BTreeMap::new();
    for p in places.iter().filter(|p| p.kind == PlaceKind::Admin) {
        if let (Some(stated), Some(&model)) = (p.population, m.by_key.get(&p.key))
            && stated > 0
        {
            #[allow(clippy::cast_precision_loss, reason = "a count of people")]
            let stated = stated as f64;
            by_level
                .entry(p.admin_level)
                .or_default()
                .push(((model - stated).abs() / stated * 100.0, model / stated));
        }
    }
    let mut out = format!(
        "Modelled population (D-123): {:.0} people in {} cells of the box, {:.0} outside every unit.\n",
        m.total, m.cells, m.outside
    );
    for (level, mut v) in by_level {
        let mut err: Vec<f64> = v.iter().map(|x| x.0).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        let ratio = v.get(v.len() / 2).map_or(0.0, |x| x.1);
        let _ = writeln!(
            out,
            "- level {level}: {} units with a stated population; |error| median {:.1} %, P90 {:.1} %; modelled/stated median {ratio:.3}",
            v.len(),
            pct(&mut err, 0.5),
            pct(&mut err, 0.9),
        );
    }
    out
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a short list"
    )]
    let i = ((v.len().saturating_sub(1)) as f64 * q).round() as usize;
    v.get(i).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{Modelled, Outcome, fill};
    use crate::build::{Place, PlaceName};
    use teigeodata_core::PlaceKind;

    fn unit(key: &str, parent: Option<&str>, population: Option<i64>) -> Place {
        Place {
            key: key.to_string(),
            kind: PlaceKind::Admin,
            admin_level: 1,
            flags: 0,
            point: (0, 0),
            names: vec![PlaceName {
                text: key.to_string(),
                lang: None,
                kind: None,
                origin: crate::build::Origin::Unknown,
            }],
            population,
            rank: 0,
            parent: parent.map(str::to_string),
            sources: Vec::new(),
        }
    }

    /// A unit's key and its box (min x, min y, max x, max y).
    type Unit<'a> = (&'a str, (i32, i32, i32, i32));

    fn level(number: u8, units: &[Unit<'_>]) -> crate::build::units::Level {
        let polys: Vec<crate::geom::Polygon> = units
            .iter()
            .map(|&(_, (x0, y0, x1, y1))| crate::geom::Polygon {
                rings: vec![vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]],
            })
            .collect();
        crate::build::units::Level {
            number,
            keys: units.iter().map(|(k, _)| (*k).to_string()).collect(),
            index: crate::geom::GridIndex::new(&polys, 100),
            polys,
        }
    }

    #[test]
    fn land_no_finer_unit_holds_counts_in_the_unit_above() {
        // m holds wards w1 and w2 on its west half; its east half is a filler's. p, beside
        // it, has no ward at all (a national park).
        let levels = [
            level(3, &[("m", (0, 0, 200, 100)), ("p", (200, 0, 300, 100))]),
            level(4, &[("w1", (0, 0, 50, 100)), ("w2", (50, 0, 100, 100))]),
        ];
        let at = |p| super::deepest_holding(&levels, p, None);
        assert_eq!(at((10, 10)), Some((1, 0)));
        assert_eq!(at((60, 10)), Some((1, 1)));
        assert_eq!(at((150, 10)), Some((0, 0)), "filler land: the local level");
        assert_eq!(at((250, 10)), Some((0, 1)), "no ward: the local level");
        assert_eq!(at((350, 10)), None);
        // The previous cell's unit is tried first only at the deepest level.
        assert_eq!(
            super::deepest_holding(&levels, (10, 10), Some((0, 0))),
            Some((1, 0))
        );
        assert_eq!(
            super::deepest_holding(&levels, (60, 10), Some((1, 0))),
            Some((1, 1))
        );

        let mut m = unit("m", None, None);
        m.admin_level = 3;
        let mut p = unit("p", None, None);
        p.admin_level = 3;
        let places = [
            m,
            p,
            unit("w1", Some("m"), None),
            unit("w2", Some("m"), None),
        ];
        let sums = vec![vec![30.0, 7.0], vec![10.0, 20.0]];
        let mut by_key = std::collections::HashMap::new();
        super::up_chains(&levels, &sums, &places, &mut by_key);
        let got = |k: &str| by_key.get(k).copied();
        assert_eq!(
            (got("m"), got("p"), got("w1"), got("w2")),
            (Some(60.0), Some(7.0), Some(10.0), Some(20.0))
        );
    }

    #[test]
    fn a_plausible_stated_population_stays_and_a_wrong_one_is_replaced() {
        let mut places = [
            unit("a", None, Some(110)),
            unit("b", None, None),
            unit("c", None, Some(1_000)),
            unit("d", None, None),
            unit("e", None, Some(70)),
        ];
        let m = Modelled {
            by_key: [
                ("a", 100.0),
                ("b", 41.6),
                ("c", 100.0),
                ("d", 0.2),
                ("e", 100.0),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            ..Modelled::default()
        };
        let rows = fill(&mut places, &m);
        let pops: Vec<Option<i64>> = places.iter().map(|p| p.population).collect();
        // a within 1.5×: stated; b none: modelled; c 10× off: modelled; d below one person:
        // none; e at 1.43×: stated.
        assert_eq!(pops, [Some(110), Some(42), Some(100), None, Some(70)]);
        assert_eq!(rows.len(), 5);
        // b and c keep the grid's count (D-351); a correction then states c's, so only b is
        // modelled.
        let outcome = Outcome {
            report: String::new(),
            rows,
        };
        let keys = |p: &[Place]| outcome.modelled(p).into_iter().collect::<Vec<_>>();
        assert_eq!(keys(&places), ["b", "c"]);
        places[2].population = Some(950);
        assert_eq!(keys(&places), ["b"]);
    }
}
