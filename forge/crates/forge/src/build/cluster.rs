//! Conflated clusters to places: kinds, admin records (D-110) and parents.

use super::Place;
use super::PlaceName;
use super::admins;
use super::corrections::{self, Correction, Outcome};
use super::naming::Here;
use super::naming::Naming;
use super::naming::place_names;
use super::sources::Rec;
use super::sources::read_clusters;
use super::sources::read_names;
use super::sources::read_records;
use super::sources::settlement_rank;
use super::units::Units;
use super::units::deepest;
use crate::aliases::rules::Rules;
use crate::build::Level;
use crate::names::Namer;
use std::collections::BTreeMap;
use std::path::Path;
use teigeodata_core::PlaceKind;
use teigeodata_format::places;
use teigeodata_names::PhoneticKey;

/// One place per cluster holding a settlement or locality record. `run` is the conflated run
/// (its order names the run's folder); `read` the sources this build may read, in priority
/// order.
/// The settlements and localities of the conflated clusters.
pub(super) struct Clustered {
    /// What the `same-as` corrections did (ADR-0036).
    pub(super) linked: Vec<(String, Outcome)>,
    pub(super) places: Vec<Place>,
    /// Clusters left out for want of a name that can be stored.
    pub(super) unnamed: usize,
    /// Places that kept their seed point because the core point lies outside every admin unit.
    pub(super) held: usize,
    /// Places left out because they lie outside every admin unit, beyond the border reach
    /// (D-201, D-239): key, kind, first name, metres outside the country's outline.
    pub(super) abroad: Vec<(String, PlaceKind, String, f64)>,
    pub(super) naming: Naming,
    pub(super) admin: AdminCounts,
}

/// How far a summit on the border may lie outside the country's units and still be the
/// country's (D-201): about 200 m, in 1e-7°. A summit sits on a ridge the border follows.
const BORDER_REACH_E7: i32 = 20_000;

/// [`BORDER_REACH_E7`] for every other current place (D-239): about 500 m. Boundaries along a
/// river are coarser than along a ridge: Nepal's border villages on the Mahakali lie up to
/// 346 m outside its outline, and the mislocated records left out start at 1.3 km.
const SETTLED_REACH_E7: i32 = 50_000;

/// A place's parent unit: the deepest holding it, or near it on the border; a former unit's is
/// its successor, the current unit one level up. `None` for a current place across the
/// border, which is the neighbour's (D-201, D-239).
#[allow(
    clippy::option_option,
    reason = "outer None: not this country's place; inner: no parent"
)]
fn parent_of(
    (kind, level, flags): (PlaceKind, u8, u32),
    point: (i32, i32),
    levels: &[Level],
    units: &admins::Units<'_>,
) -> Option<Option<String>> {
    let reach = if kind == PlaceKind::Natural {
        BORDER_REACH_E7
    } else {
        SETTLED_REACH_E7
    };
    if flags & places::FLAG_FORMER == 0 {
        border_parent(levels, point, reach).map(Some)
    } else {
        Some(units.successor(level, point))
    }
}

/// The deepest unit holding a place: at its point, else at a probe within `reach` of it (a
/// summit or a village on the border line), nearest probes first; `None` for one across the
/// border.
fn border_parent(levels: &[Level], (lon, lat): (i32, i32), reach: i32) -> Option<String> {
    const DIRS: [(i32, i32); 8] = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ];
    deepest(levels, (lon, lat)).or_else(|| {
        [reach / 4, reach / 2, reach].into_iter().find_map(|r| {
            DIRS.iter().find_map(|&(dx, dy)| {
                deepest(
                    levels,
                    (lon.saturating_add(dx * r), lat.saturating_add(dy * r)),
                )
            })
        })
    })
}

/// How far `point` lies outside the country, in metres: the coarsest level's units cover it, so
/// the nearest of their outlines is the border.
fn outside_m(levels: &[Level], point: (i32, i32)) -> f64 {
    levels.first().map_or(f64::INFINITY, |l| {
        l.polys
            .iter()
            .map(|p| p.distance_m(point))
            .fold(f64::INFINITY, f64::min)
    })
}

/// Writes `abroad-<build>.tsv` beside the pack, farthest first, and returns the summary line:
/// the places left out as the neighbour's (D-201, D-239).
pub(super) fn abroad_report(
    abroad: &[(String, PlaceKind, String, f64)],
    out_dir: &Path,
    build: &str,
) -> Result<String, String> {
    use std::fmt::Write as _;
    let mut rows: Vec<_> = abroad.iter().collect();
    rows.sort_by(|a, b| b.3.total_cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    let mut tsv = String::from(
        "# Places outside every admin unit, beyond the border reach: the neighbour's (D-201, D-239).\nplace\tkind\tname\tmetres outside\n",
    );
    for (key, kind, name, m) in rows {
        let _ = writeln!(tsv, "{key}\t{kind:?}\t{name}\t{m:.0}");
    }
    let file = out_dir.join(format!("abroad-{build}.tsv"));
    std::fs::write(&file, tsv).map_err(|e| format!("{}: {e}", file.display()))?;
    if abroad.is_empty() {
        return Ok(String::new());
    }
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for (_, kind, _, _) in abroad {
        *by_kind
            .entry(format!("{kind:?}").to_lowercase())
            .or_default() += 1;
    }
    let kinds: Vec<String> = by_kind.iter().map(|(k, n)| format!("{n} {k}")).collect();
    Ok(format!(
        "Left out as the neighbour's (D-201, D-239): {} places outside every admin unit beyond the border reach (about 200 m for a summit, 500 m for any other place; {}), listed in `abroad-{build}.tsv`.\n",
        abroad.len(),
        kinds.join(", ")
    ))
}

/// What became of the clusters of admin records (D-110).
#[derive(Default)]
pub(super) struct AdminCounts {
    pub(super) links: Vec<admins::Link>,
    /// Linked clusters by the level of their unit (0: the country).
    pub(super) linked: BTreeMap<u8, usize>,
    /// Former units kept as places.
    pub(super) former: usize,
    /// Undifferentiated divisions kept as localities.
    pub(super) divisions: usize,
    /// Former units deeper than `MAX_FORMER_LEVEL`, left out.
    pub(super) parts: usize,
    /// Clusters naming no unit that are neither former nor divisions, left out.
    pub(super) unmatched: usize,
    /// Names a link did not carry (D-060's collision rule).
    pub(super) collisions: usize,
    /// Names the links added to their units.
    pub(super) added: usize,
}

impl AdminCounts {
    pub(super) fn line(&self) -> String {
        let linked: Vec<String> = self
            .linked
            .iter()
            .map(|(l, n)| format!("level {l}: {n}"))
            .collect();
        format!(
            "Admin records (D-110): {} clusters linked to their unit ({}), {} names not carried; {} former units and {} divisions kept as places; {} deeper former parts and {} unmatched left out.\nNames added to admin units by links: {}.\n",
            self.linked.values().sum::<usize>(),
            linked.join(", "),
            self.collisions,
            self.former,
            self.divisions,
            self.parts,
            self.unmatched,
            self.added
        )
    }
}

/// Gives each linked unit (or the country) its records' keys and names; returns how many
/// names were added. Unit indexes are into the admin places, which `places` starts with.
pub(super) fn link_admins(places: &mut [Place], links: Vec<admins::Link>) -> usize {
    let country = places.iter().position(|p| p.kind == PlaceKind::Country);
    let mut added = 0;
    for link in links {
        if let Some(p) = link.unit.or(country).and_then(|i| places.get_mut(i)) {
            added += admins::apply(p, link);
        }
    }
    added
}

/// A cluster's kind, admin level and flags; `None` for a cluster that is not a place of its
/// own (an admin cluster linked to its unit, D-110, or a kind the build does not keep). A
/// cluster with a settlement is a settlement, then a locality, an admin unit, a natural feature.
pub(super) fn place_kind(
    recs: &[(&Rec, &str, usize)],
    names: &[BTreeMap<String, Vec<PlaceName>>],
    at: (u32, (i32, i32)),
    units: (&admins::Units<'_>, &Rules<'_>),
    admin: &mut AdminCounts,
) -> Option<(PlaceKind, u8, u32)> {
    if recs.iter().any(|(r, _, _)| r.kind == "populated_place") {
        return Some((PlaceKind::Settlement, 0, 0));
    }
    if recs.iter().any(|(r, _, _)| r.kind == "locality") {
        return Some((PlaceKind::Locality, 0, 0));
    }
    if !recs.iter().any(|(r, _, _)| r.kind == "admin") {
        // A summit, hill, volcano or pass (D-201).
        return recs.iter().any(|(r, _, _)| r.kind == "natural").then_some((
            PlaceKind::Natural,
            0,
            0,
        ));
    }
    match admin_cluster(recs, names, at, units, admin) {
        Admin::Linked(link) => {
            admin.links.push(link);
            None
        }
        Admin::Former(n) => Some((PlaceKind::Admin, n, places::FLAG_FORMER)),
        Admin::Division => Some((PlaceKind::Locality, 0, 0)),
        Admin::Left => None,
    }
}

/// What an admin cluster becomes.
pub(super) enum Admin {
    Linked(admins::Link),
    Former(u8),
    Division,
    Left,
}

/// Classifies a cluster of admin records and, if it names a current unit, the link (D-110).
pub(super) fn admin_cluster(
    recs: &[(&Rec, &str, usize)],
    names: &[BTreeMap<String, Vec<PlaceName>>],
    (owner, seed): (u32, (i32, i32)),
    (units, rules): (&admins::Units<'_>, &Rules<'_>),
    counts: &mut AdminCounts,
) -> Admin {
    use admins::Class;
    let class = Class::of_cluster(recs.iter().map(|(r, _, _)| r.class.as_str()));
    if let Class::Former(n) = class
        && n > admins::MAX_FORMER_LEVEL
    {
        counts.parts += 1;
        return Admin::Left;
    }
    let mut all: Vec<PlaceName> = Vec::new();
    for &(r, id, s) in recs {
        if !r.name.is_empty() {
            all.push(PlaceName {
                text: r.name.clone(),
                lang: None,
                kind: None,
                origin: super::sources::origin_of(id),
            });
        }
        all.extend(
            names
                .get(s)
                .and_then(|m| m.get(id))
                .into_iter()
                .flatten()
                .cloned(),
        );
    }
    let texts: Vec<&str> = all.iter().map(|n| n.text.as_str()).collect();
    let unit = if class == Class::Country {
        Some(None)
    } else {
        units.find(class, seed, &texts, rules).map(Some)
    };
    let Some(unit) = unit else {
        return match class {
            Class::Former(n) => {
                counts.former += 1;
                Admin::Former(n)
            }
            Class::Division => {
                counts.divisions += 1;
                Admin::Division
            }
            _ => {
                counts.unmatched += 1;
                Admin::Left
            }
        };
    };
    // The unit's names are the anchor; a carried name it lacks must not name another place.
    let target = unit.and_then(|i| units.place(i));
    if let Some(p) = target {
        let known = rules.anchor(p.names.iter().map(|n| n.text.as_str()));
        let before = all.len();
        all.retain(|n| {
            Rules::fold_known(&known, &n.text) || rules.collides(owner, p.point, &n.text).is_none()
        });
        counts.collisions += before - all.len();
    } else if unit.is_none() {
        // The country: its records' primary names are its labels.
        let labels = rules.anchor(recs.iter().map(|(r, _, _)| r.name.as_str()));
        let before = all.len();
        all.retain(|n| rules.country_keeps(&labels, &n.text).is_ok());
        counts.collisions += before - all.len();
    }
    *counts
        .linked
        .entry(target.map_or(0, |p| p.admin_level))
        .or_default() += 1;
    Admin::Linked(admins::Link {
        unit,
        sources: recs.iter().map(|(_, id, _)| (*id).to_string()).collect(),
        names: all,
        population: recs.iter().filter_map(|(r, _, _)| r.population).max(),
    })
}

pub(super) fn cluster_places(
    build: &Path,
    run: &[String],
    read: &[String],
    units: &Units,
    core: &BTreeMap<String, (i32, i32)>,
    (display, phonetic): (&Namer, Option<PhoneticKey>),
    (fixes, lite): (&[Correction], bool),
) -> Result<Clustered, String> {
    let levels = &units.levels;
    let mut records: Vec<BTreeMap<String, Rec>> = Vec::new();
    let mut names: Vec<BTreeMap<String, Vec<PlaceName>>> = Vec::new();
    for s in read {
        records.push(read_records(build, s)?);
        names.push(read_names(build, s)?);
    }
    let path = build
        .join("conflate")
        .join(run.join("+"))
        .join("clusters.parquet");
    let mut clusters = read_clusters(&path, &records)?;
    let linked = corrections::link_clusters(fixes, &mut clusters, &records, lite);
    // Every cluster with a point, its records in source priority; the primary names of all of
    // them and of the admin units feed the collision rule (D-060).
    let mut rules = Rules::new(phonetic, Some(display));
    for p in &units.places {
        if let Some(n) = p.names.first() {
            rules.add_admin(&n.text);
        }
    }
    let groups = groups(clusters, &records, &mut rules)?;
    rules.finish();
    let unit_index = admins::Units::new(&units.places, levels, &rules);
    let mut admin = AdminCounts::default();
    let (mut out, mut unnamed, mut held, mut abroad) = (Vec::new(), 0_usize, 0_usize, Vec::new());
    let mut naming = Naming::default();
    for (
        owner,
        Group {
            cluster,
            members,
            seed,
        },
    ) in (0_u32..).zip(groups)
    {
        let recs: Vec<(&Rec, &str, usize)> = members
            .iter()
            .filter_map(|(s, id)| Some((records.get(*s)?.get(id)?, id.as_str(), *s)))
            .collect();
        let Some((kind, level, flags)) = place_kind(
            &recs,
            &names,
            (owner, seed),
            (&unit_index, &rules),
            &mut admin,
        ) else {
            continue;
        };
        // The consensus point of the `core` stage, when the build reads one. A core that would
        // take the place out of every admin unit its seed point is in (across the border, as
        // buildings on the far side pull it) is refused: the place keeps its seed point.
        let point = match core.get(&cluster) {
            Some(&c) if deepest(levels, c).is_some() || deepest(levels, seed).is_none() => c,
            Some(_) => {
                held += 1;
                seed
            }
            None => seed,
        };
        let here = Here {
            cluster: &cluster,
            owner,
            seed,
        };
        let Some(all) = place_names(&here, &recs, &names, (display, &rules), &mut naming) else {
            unnamed += 1;
            continue;
        };
        let Some(parent) = parent_of((kind, level, flags), point, levels, &unit_index) else {
            let name = all.first().map(|n| n.text.clone()).unwrap_or_default();
            abroad.push((cluster, kind, name, outside_m(levels, point)));
            continue;
        };
        out.push(Place {
            key: cluster,
            kind,
            admin_level: level,
            flags,
            point,
            names: all,
            population: recs.iter().find_map(|(r, _, _)| r.population),
            rank: recs
                .iter()
                .map(|(r, _, _)| settlement_rank(&r.class))
                .max()
                .unwrap_or(0),
            parent,
            sources: members.into_iter().map(|(_, id)| id).collect(),
        });
    }
    Ok(Clustered {
        linked,
        places: out,
        unnamed,
        held,
        abroad,
        naming,
        admin,
    })
}

/// Every cluster with a point, its records in source priority; registers each one's primary
/// name with the collision rule (D-060).
pub(super) fn groups(
    clusters: BTreeMap<String, Vec<(usize, String)>>,
    records: &[BTreeMap<String, Rec>],
    rules: &mut Rules<'_>,
) -> Result<Vec<Group>, String> {
    let mut groups: Vec<Group> = Vec::new();
    for (cluster, mut members) in clusters {
        members.sort();
        let recs = members
            .iter()
            .filter_map(|(s, id)| records.get(*s)?.get(id));
        let (mut point, mut primary) = (None, None);
        for r in recs {
            point = point.or(r.point);
            if primary.is_none() && !r.name.is_empty() {
                primary = Some(r.name.as_str());
            }
        }
        // The first source's record with a point gives the point.
        let Some(point) = point else { continue };
        let owner = u32::try_from(groups.len()).map_err(|_| "too many clusters")?;
        if let Some(n) = primary {
            rules.add_place(owner, point, n);
        }
        groups.push(Group {
            cluster,
            members,
            seed: point,
        });
    }
    Ok(groups)
}

/// A cluster with a point.
pub(super) struct Group {
    pub(super) cluster: String,
    /// (source index, record ID), in source priority.
    pub(super) members: Vec<(usize, String)>,
    /// The first source's point.
    pub(super) seed: (i32, i32),
}
