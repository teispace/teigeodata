//! `forge world --towns <cc>` (TG-15, D-258, D-259): a country's official districts and towns
//! in the world part, from `data/countries/<cc>/towns.yaml` (P16: countries are data).
//!
//! - **Districts** are code-only units under the world part's first-level unit their list's
//!   state code names (by the Wikidata item that unit carries). A district's point is its
//!   `GeoNames` unit's (`ADM2`, through a pinned code → `GeoNames` crosswalk), else the middle of
//!   its located towns, else it is left out and its towns hang under the state.
//! - **Towns** are the official list's rows, each placed at a populated place of the points
//!   source found by name **within its district** (the district's `ADM2`), or within its state
//!   when a district has no crosswalk. A name matches on its phonetic key (the profile's) of the
//!   name without spaces, the body-type words cut; a compound name ("Chamoli Gopeshwar") on
//!   one of its parts when the whole finds nothing. Several candidates: the one populated or a
//!   seat (`PPLA*`, `PPLC`), when exactly one is. **No point is guessed** (india-sources §3.8):
//!   an ambiguous or unmatched town, one whose place lies outside its state's outline, or one
//!   whose place another town took first, is reported and left out.
//!
//! Memory is bounded by the list, not the points source: the dump is streamed once and only
//! the places whose key some town asks for are kept.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use teigeodata_names::{PhoneticKey, join_key};

use super::world::{City, for_each_record};
use crate::lock::Lock;
use crate::read::table::for_each_row;

const SCHEMA: &str = "teigeodata/towns/v1";

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    states: States,
    districts: Districts,
    towns: TownList,
    /// The populated places (a `GeoNames` dump).
    points: String,
    /// Gate G-town: the share of towns located, at least (a floor that only rises).
    floor: f64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct States {
    source: String,
    code: String,
    /// Each list code to the Wikidata item of the world part's unit it is.
    units: BTreeMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Districts {
    source: String,
    code: String,
    name: String,
    parent: String,
    /// A `?item ?code ?gn` snapshot: each district code's Wikidata item and `GeoNames` unit.
    crosswalk: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TownList {
    source: String,
    code: String,
    name: String,
    state: String,
    coverage: Coverage,
    #[serde(default)]
    strip: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    source: String,
    code: String,
    district: String,
}

/// A district row.
pub(super) struct District {
    pub(super) key: String,
    pub(super) name: String,
    /// The Wikidata item of its state's world unit.
    pub(super) state: String,
    pub(super) point: (i32, i32),
    /// Its other source keys (`wikidata:`, `geonames:`).
    pub(super) refs: Vec<String>,
}

/// A located town.
pub(super) struct Town {
    pub(super) key: String,
    /// The list's name, type words cut.
    pub(super) name: String,
    /// Its district's row key, when the district is kept.
    pub(super) district: Option<String>,
    /// The Wikidata item of its state's world unit.
    pub(super) state: String,
    /// The populated place it is placed at.
    pub(super) city: City,
}

/// What a country adds to the world part.
pub(super) struct Built {
    pub(super) districts: Vec<District>,
    pub(super) towns: Vec<Town>,
    pub(super) report: String,
}

/// The pinned sources a country's towns read, for the licence gate and the notices.
pub(super) fn sources(root: &Path, cc: &str) -> Result<Vec<String>, String> {
    let f = read(root, cc)?;
    Ok(vec![
        f.states.source,
        f.districts.source,
        f.districts.crosswalk,
        f.towns.source,
        f.towns.coverage.source,
        f.points,
    ])
}

fn read(root: &Path, cc: &str) -> Result<File, String> {
    let path = root.join(format!("data/countries/{cc}/towns.yaml"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let f: File = serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if f.schema != SCHEMA {
        return Err(format!("{}: schema is not {SCHEMA}", path.display()));
    }
    if !(0.0..=1.0).contains(&f.floor) {
        return Err(format!("{}: floor is a share, 0 to 1", path.display()));
    }
    Ok(f)
}

/// A `GeoNames` unit: its admin codes and point.
type Adm2 = ((String, String), (i32, i32));

/// How a town was placed, or why not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    District,
    State,
    Ambiguous,
    NoMatch,
    OutsideState,
    Taken,
}

impl Outcome {
    const fn label(self) -> &'static str {
        match self {
            Self::District => "placed in its district",
            Self::State => "placed in its state (no district crosswalk)",
            Self::Ambiguous => "ambiguous",
            Self::NoMatch => "no name match",
            Self::OutsideState => "outside its state's outline",
            Self::Taken => "its place taken by another town",
        }
    }
}

/// A candidate place: its index in the kept places, its admin codes, and whether it is
/// populated or a seat.
struct Cand {
    i: usize,
    admin: (String, String),
    notable: bool,
}

/// Picks one place of `pool`: the only one, else the only populated place or seat.
fn pick(pool: &[&Cand]) -> Option<usize> {
    if let [one] = pool {
        return Some(one.i);
    }
    let mut notable = pool.iter().filter(|c| c.notable);
    match (notable.next(), notable.next()) {
        (Some(one), None) => Some(one.i),
        _ => None,
    }
}

/// Places one town: within its districts' `GeoNames` units when it has any, else within its
/// state's admin-1 codes when a district lacks a crosswalk. Returns the place and how.
fn place(
    cands: &[&Cand],
    districts: &BTreeSet<(String, String)>,
    unlinked: bool,
    state_a1: &BTreeSet<String>,
) -> (Option<usize>, Outcome) {
    if cands.is_empty() {
        return (None, Outcome::NoMatch);
    }
    let in_d: Vec<&Cand> = cands
        .iter()
        .copied()
        .filter(|c| districts.contains(&c.admin))
        .collect();
    if !in_d.is_empty() {
        return pick(&in_d).map_or((None, Outcome::Ambiguous), |i| (Some(i), Outcome::District));
    }
    if unlinked {
        let in_s: Vec<&Cand> = cands
            .iter()
            .copied()
            .filter(|c| state_a1.contains(&c.admin.0))
            .collect();
        if let Some(i) = pick(&in_s) {
            return (Some(i), Outcome::State);
        }
    }
    (None, Outcome::Ambiguous)
}

/// The match keys of a name: the phonetic key (or the join key) of the whole name, then of each
/// part of a compound name.
fn name_keys(name: &str, phonetic: Option<PhoneticKey>) -> (String, Vec<String>) {
    let key = |s: &str| {
        let j = join_key(s);
        phonetic.map_or_else(|| j.clone(), |p| p.key(&j))
    };
    let parts = name
        .split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .filter(|p| p.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        .map(key)
        .filter(|k| !k.is_empty())
        .collect();
    (key(name), parts)
}

/// Cuts each of `strip` (whole words, case-insensitive, wherever it stands) from `name` and
/// tidies what is left ("Municipal Council Kupwara" → "Kupwara").
fn cut_types(name: &str, strip: &[String]) -> String {
    let mut words: Vec<&str> = name.split_whitespace().collect();
    for s in strip {
        let t: Vec<&str> = s.split_whitespace().collect();
        if t.is_empty() {
            continue;
        }
        while let Some(at) = words.windows(t.len()).position(|w| {
            w.iter().zip(&t).all(|(a, b)| {
                a.trim_matches(|c: char| !c.is_alphanumeric())
                    .eq_ignore_ascii_case(b)
            })
        }) {
            words.drain(at..at + t.len());
        }
    }
    let out = words.join(" ");
    let out = out.trim_matches(|c: char| c == '-' || c == ',' || c.is_whitespace());
    if out.is_empty() {
        name.trim().to_string()
    } else {
        out.to_string()
    }
}

/// Builds a country's districts and towns. `inside(item, point)` says whether a point lies in the
/// world unit carrying Wikidata item `item` (`None`: no such unit).
#[allow(clippy::too_many_lines, reason = "the stage's steps, in order")]
pub(super) fn build(
    root: &Path,
    lock: &Lock,
    cc: &str,
    inside: impl Fn(&str, (i32, i32)) -> Option<bool>,
) -> Result<Built, String> {
    let f = read(root, cc)?;
    let phonetic = super::Profile::read(root, cc)?.phonetic;
    let path = |id: &str| lock.path(root, id);

    // Every state of the list is mapped, so a new one fails the build until reviewed.
    let mut listed = BTreeSet::new();
    for_each_row(&path(&f.states.source)?, &[&f.states.code], |r| {
        listed.insert(r.first().copied().unwrap_or_default().to_string());
        Ok(())
    })?;
    if let Some(s) = listed.iter().find(|s| !f.states.units.contains_key(*s)) {
        return Err(format!(
            "{cc}: state {s} of {} has no unit in towns.yaml",
            f.states.source
        ));
    }

    // The crosswalk: each district code's Wikidata item and GeoNames unit, when it has exactly one.
    let mut walk: BTreeMap<String, (String, Option<u64>)> = BTreeMap::new();
    let mut two = BTreeSet::new();
    let text = std::fs::read_to_string(path(&f.districts.crosswalk)?).map_err(|e| e.to_string())?;
    for line in text.lines().skip(1) {
        let mut c = line.split('\t');
        let (Some(item), Some(code), gn) = (c.next(), c.next(), c.next()) else {
            continue;
        };
        let q = item
            .trim_start_matches("<http://www.wikidata.org/entity/")
            .trim_end_matches('>');
        let code = code.trim_matches('"').to_string();
        let gn = gn.and_then(|g| g.trim_matches('"').parse::<u64>().ok());
        match walk.get(&code) {
            Some((_, g)) if *g != gn => {
                two.insert(code);
            }
            Some(_) => {}
            None => {
                walk.insert(code, (q.to_string(), gn));
            }
        }
    }
    for c in &two {
        if let Some(e) = walk.get_mut(c) {
            e.1 = None;
        }
    }

    let mut districts: Vec<(String, String, String)> = Vec::new();
    for_each_row(
        &path(&f.districts.source)?,
        &[&f.districts.code, &f.districts.name, &f.districts.parent],
        |r| {
            if let [code, name, state] = r {
                districts.push(((*code).into(), (*name).into(), (*state).into()));
            }
            Ok(())
        },
    )?;
    let mut cover: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for_each_row(
        &path(&f.towns.coverage.source)?,
        &[&f.towns.coverage.code, &f.towns.coverage.district],
        |r| {
            if let [t, d] = r
                && !d.is_empty()
                && *d != "0"
            {
                cover.entry((*t).into()).or_default().insert((*d).into());
            }
            Ok(())
        },
    )?;
    let mut list: Vec<(String, String, String)> = Vec::new();
    for_each_row(
        &path(&f.towns.source)?,
        &[&f.towns.code, &f.towns.name, &f.towns.state],
        |r| {
            if let [code, name, state] = r {
                list.push((
                    (*code).into(),
                    cut_types(name, &f.towns.strip),
                    (*state).into(),
                ));
            }
            Ok(())
        },
    )?;

    // The keys towns ask for, then one pass over the dump keeping only those places and the
    // crosswalk's units.
    let keys: Vec<(String, Vec<String>)> = list.iter().map(|t| name_keys(&t.1, phonetic)).collect();
    let wanted: BTreeSet<&str> = keys
        .iter()
        .flat_map(|(k, p)| std::iter::once(k).chain(p))
        .map(String::as_str)
        .collect();
    let units_wanted: BTreeSet<u64> = walk.values().filter_map(|v| v.1).collect();
    let mut kept: Vec<City> = Vec::new();
    let mut by_key: HashMap<String, Vec<Cand>> = HashMap::new();
    let mut adm2: HashMap<u64, Adm2> = HashMap::new();
    for_each_record(&path(&f.points)?, |r| {
        let admin = (r.admin1.to_string(), r.admin2.to_string());
        if r.class == "A" && r.code == "ADM2" && units_wanted.contains(&r.city.id) {
            adm2.insert(r.city.id, (admin, r.city.point));
            return Ok(());
        }
        if r.class != "P" {
            return Ok(());
        }
        let mut hit = BTreeSet::new();
        for n in [r.city.name.as_str(), r.city.ascii.as_str()]
            .into_iter()
            .chain(r.alternates.split(',').filter(|a| a.is_ascii()))
        {
            let k = name_keys(n, phonetic).0;
            if wanted.contains(k.as_str()) {
                hit.insert(k);
            }
        }
        if hit.is_empty() {
            return Ok(());
        }
        let notable = r.city.population.is_some() || r.code.starts_with("PPLA") || r.code == "PPLC";
        let i = kept.len();
        kept.push(r.city);
        for k in hit {
            by_key.entry(k).or_default().push(Cand {
                i,
                admin: admin.clone(),
                notable,
            });
        }
        Ok(())
    })?;

    // Each district's GeoNames unit; each state's admin-1 codes, from its linked districts.
    let unit_of = |d: &str| walk.get(d).and_then(|w| w.1).and_then(|g| adm2.get(&g));
    let mut state_a1: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (code, _, state) in &districts {
        if let Some(((a1, _), _)) = unit_of(code) {
            state_a1
                .entry(state.as_str())
                .or_default()
                .insert(a1.clone());
        }
    }
    let empty = BTreeSet::new();
    let mut outcomes: BTreeMap<Outcome, usize> = BTreeMap::new();
    let mut by_state: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut taken: BTreeSet<usize> = BTreeSet::new();
    let mut placed: Vec<(usize, usize)> = Vec::new(); // (list index, kept index)
    for (ti, (t, (whole, parts))) in list.iter().zip(&keys).enumerate() {
        let ds = cover.get(&t.0);
        let units: BTreeSet<(String, String)> = ds
            .into_iter()
            .flatten()
            .filter_map(|d| unit_of(d).map(|u| u.0.clone()))
            .collect();
        let unlinked = ds.is_none_or(|ds| ds.iter().any(|d| unit_of(d).is_none()));
        let a1 = state_a1.get(t.2.as_str()).unwrap_or(&empty);
        let find = |k: &String| by_key.get(k).into_iter().flatten().collect::<Vec<&Cand>>();
        let mut got = place(&find(whole), &units, unlinked, a1);
        if got.1 == Outcome::NoMatch {
            let mut all: Vec<&Cand> = parts.iter().flat_map(find).collect();
            all.sort_by_key(|c| c.i);
            all.dedup_by_key(|c| c.i);
            got = place(&all, &units, unlinked, a1);
        }
        let state = f
            .states
            .units
            .get(&t.2)
            .map(String::as_str)
            .unwrap_or_default();
        let outcome = match got {
            (Some(i), how) => match kept.get(i).and_then(|c| inside(state, c.point)) {
                Some(true) if taken.insert(i) => {
                    placed.push((ti, i));
                    how
                }
                Some(true) => Outcome::Taken,
                _ => Outcome::OutsideState,
            },
            (None, how) => how,
        };
        *outcomes.entry(outcome).or_default() += 1;
        let e = by_state.entry(t.2.as_str()).or_default();
        e.0 += 1;
        e.1 += usize::from(matches!(outcome, Outcome::District | Outcome::State));
    }

    // Districts: the GeoNames unit's point, else the middle of their located towns.
    let mut towns_in: BTreeMap<&str, Vec<(i32, i32)>> = BTreeMap::new();
    for &(t, i) in &placed {
        if let (Some(t), Some(c)) = (list.get(t), kept.get(i))
            && let Some(ds) = cover.get(&t.0)
            && let Some(d) = ds.first()
        {
            towns_in.entry(d.as_str()).or_default().push(c.point);
        }
    }
    let (mut by_unit, mut by_towns, mut dropped) = (0, 0, 0);
    let mut out_d = Vec::new();
    let mut kept_d = BTreeSet::new();
    for (code, name, state) in &districts {
        let Some(item) = f.states.units.get(state) else {
            dropped += 1;
            continue;
        };
        // The unit's point, else (none, or outside the state) the middle of its towns.
        let within = |p: &(i32, i32)| inside(item, *p) == Some(true);
        let point = if let Some(p) = unit_of(code).map(|(_, p)| *p).filter(within) {
            by_unit += 1;
            p
        } else if let Some(p) = towns_in
            .get(code.as_str())
            .and_then(|ps| middle(ps))
            .filter(within)
        {
            by_towns += 1;
            p
        } else {
            dropped += 1;
            continue;
        };
        let key = format!("{}:{code}", f.districts.source);
        let mut refs = Vec::new();
        if let Some((q, g)) = walk.get(code) {
            refs.push(format!("wikidata:{q}"));
            refs.extend(g.map(|g| format!("geonames:{g}")));
        }
        kept_d.insert(code.as_str());
        out_d.push(District {
            key,
            name: name.clone(),
            state: item.clone(),
            point,
            refs,
        });
    }
    let mut out_t = Vec::new();
    for (t, i) in placed {
        let (Some(t), Some(city)) = (list.get(t), kept.get(i)) else {
            continue;
        };
        let district = cover
            .get(&t.0)
            .and_then(|ds| ds.iter().find(|d| kept_d.contains(d.as_str())))
            .map(|d| format!("{}:{d}", f.districts.source));
        out_t.push(Town {
            key: format!("{}:{}", f.towns.source, t.0),
            name: t.1.clone(),
            district,
            state: f.states.units.get(&t.2).cloned().unwrap_or_default(),
            city: city.clone(),
        });
    }

    let n = list.len();
    let located = out_t.len();
    let mut report = format!(
        "- Towns of `{cc}` (D-259): {} districts ({by_unit} at their GeoNames unit, {by_towns} at \
         the middle of their towns, {dropped} left out); {n} towns listed, {located} placed \
         ({:.1}%, floor {:.1}%):",
        out_d.len(),
        share(located, n) * 100.0,
        f.floor * 100.0
    );
    for (o, k) in &outcomes {
        let _ = write!(report, " {k} {},", o.label());
    }
    report.pop();
    report.push('.');
    let low: Vec<String> = by_state
        .iter()
        .filter(|(_, (n, p))| share(*p, *n) < f.floor)
        .map(|(s, (n, p))| format!("{s} {p}/{n}"))
        .collect();
    if !low.is_empty() {
        let _ = write!(
            report,
            " States below the floor (list code, placed/listed): {}.",
            low.join(", ")
        );
    }
    report.push('\n');
    if share(located, n) < f.floor {
        return Err(format!(
            "G-town: {cc}: {located} of {n} towns placed, below the floor {}\n{report}",
            f.floor
        ));
    }
    Ok(Built {
        districts: out_d,
        towns: out_t,
        report,
    })
}

#[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
fn share(a: usize, b: usize) -> f64 {
    if b == 0 { 1.0 } else { a as f64 / b as f64 }
}

/// The component-wise median of points.
fn middle(ps: &[(i32, i32)]) -> Option<(i32, i32)> {
    let mut xs: Vec<i32> = ps.iter().map(|p| p.0).collect();
    let mut ys: Vec<i32> = ps.iter().map(|p| p.1).collect();
    xs.sort_unstable();
    ys.sort_unstable();
    Some((*xs.get(xs.len() / 2)?, *ys.get(ys.len() / 2)?))
}

#[cfg(test)]
mod tests {
    use super::{Cand, Outcome, cut_types, middle, pick, place};
    use std::collections::BTreeSet;

    fn cand(i: usize, a1: &str, a2: &str, notable: bool) -> Cand {
        Cand {
            i,
            admin: (a1.into(), a2.into()),
            notable,
        }
    }

    #[test]
    fn body_types_cut_wherever_they_stand() {
        let strip = ["Municipal Council".to_string(), "Municipality".to_string()];
        assert_eq!(cut_types("Municipal Council Kupwara", &strip), "Kupwara");
        assert_eq!(cut_types("Jammu municipal council", &strip), "Jammu");
        assert_eq!(cut_types("Dharamshala", &strip), "Dharamshala");
        // A name that is only a type keeps itself, never empty.
        assert_eq!(cut_types("Municipality", &strip), "Municipality");
        // Whole words only: a name holding the word inside another is untouched.
        assert_eq!(cut_types("Municipalityganj", &strip), "Municipalityganj");
    }

    #[test]
    fn one_place_or_the_one_notable_else_none() {
        let (a, b) = (cand(1, "10", "1", false), cand(2, "10", "1", true));
        assert_eq!(pick(&[&a]), Some(1));
        assert_eq!(pick(&[&a, &b]), Some(2));
        let c = cand(3, "10", "1", true);
        assert_eq!(pick(&[&a, &b, &c]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn a_town_is_placed_in_its_district_never_a_namesake_elsewhere() {
        let mine = BTreeSet::from([("10".to_string(), "1".to_string())]);
        let state = BTreeSet::from(["10".to_string()]);
        let here = cand(1, "10", "1", false);
        let namesake = cand(2, "10", "2", true);
        assert_eq!(
            place(&[&here, &namesake], &mine, false, &state),
            (Some(1), Outcome::District)
        );
        // Only a namesake in another district: not placed, though it is the state's one match.
        assert_eq!(
            place(&[&namesake], &mine, false, &state),
            (None, Outcome::Ambiguous)
        );
        // A town of a district with no crosswalk falls back to its state's codes.
        assert_eq!(
            place(&[&namesake], &BTreeSet::new(), true, &state),
            (Some(2), Outcome::State)
        );
        let abroad = cand(3, "20", "1", false);
        assert_eq!(
            place(&[&abroad], &BTreeSet::new(), true, &state),
            (None, Outcome::Ambiguous)
        );
        assert_eq!(place(&[], &mine, false, &state), (None, Outcome::NoMatch));
    }

    #[test]
    fn middle_is_the_component_median() {
        assert_eq!(middle(&[(0, 0), (10, 100), (2, 3)]), Some((2, 3)));
        assert_eq!(middle(&[]), None);
    }
}
