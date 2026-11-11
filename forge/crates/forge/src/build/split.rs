//! Split builds (D-191, [split builds](../../../../../docs/02-research/notes/split-builds.md)):
//! one pack per unit of an admin level, each holding its own places and every ancestor of them
//! as a row (ancestor stubs, D-143), written by the same pack stage as the country pack.
//!
//! `data/countries/<cc>/split.yaml` names the level; each unit of that level with an ISO 3166-2
//! code in `iso3166.yaml` gets the address `<cc>-<code>` (lowercased, the country prefix dropped,
//! D-197), or, at a level ISO does not code, the file lists each unit's address code. A part
//! holds:
//! - its own places: those whose chain of parents reaches the unit (the unit included);
//! - every ancestor of them (the stubs);
//! - the timezone zones whose box meets the part's box, with their zone places;
//! - the admin polygons of its rows, on only the arcs they use;
//! - the postal codes of its own places, and its own places' heights;
//! - the redirects to its own places, and every tombstone.
//!
//! IDs are the country pack's. After writing, [`check`] proves each part against the country
//! pack: every own place has the same ID, point, canonical name and chain of ancestors (by ID),
//! and every place of the country pack is owned by exactly one part or is an ancestor only.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use teigeodata_core::PlaceKind;
use teigeodata_format::hierarchy::Hierarchy;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::iso_codes::IsoCodes;
use teigeodata_format::names::Names;
use teigeodata_format::place_tz::PlaceTz;
use teigeodata_format::places::Places;
use teigeodata_format::{Kind, Pack};

use super::pack::{self, Built, Input};
use super::units::Level;
use super::zones::Zones;
use super::{Place, postal};
use crate::geom::{Polygon, clip_ring};

/// `data/countries/<cc>/split.yaml`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitFile {
    schema: String,
    level: u8,
    source: Option<String>,
    units: Option<BTreeMap<String, String>>,
}

/// The split a build follows.
#[derive(Debug, Clone)]
pub(crate) struct Split {
    /// The admin level whose units are the parts.
    level: u8,
    /// Where the codes come from.
    source: String,
    /// Each unit's address code, by its place key (`admin:NP03` → `p3`).
    units: BTreeMap<String, String>,
    /// Whether the units are `iso3166.yaml`'s, of any level: the build keeps those at `level`.
    from_iso: bool,
}

impl Split {
    /// The split as it applies to `places`: units from `iso3166.yaml` are only those of its
    /// level.
    fn for_places(&self, places: &[Place]) -> Self {
        if !self.from_iso {
            return self.clone();
        }
        let level: BTreeMap<&str, u8> = places
            .iter()
            .map(|p| (p.key.as_str(), p.admin_level))
            .collect();
        Self {
            units: self
                .units
                .iter()
                .filter(|(k, _)| level.get(k.as_str()) == Some(&self.level))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            ..self.clone()
        }
    }
}

const SCHEMA: &str = "teigeodata/split/v1";

/// Reads the country's split, checking its schema, its source and every code; without units
/// of its own, the units and their codes are `iso3166.yaml`'s.
pub(crate) fn read(root: &Path, cc: &str) -> Result<Split, String> {
    let path = root.join("data/countries").join(cc).join("split.yaml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let f: SplitFile =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if f.schema != SCHEMA {
        return Err(format!("{}: schema must be {SCHEMA}", path.display()));
    }
    let s = match (f.source, f.units) {
        (Some(source), Some(units)) => Split {
            level: f.level,
            source,
            units,
            from_iso: false,
        },
        (None, None) => {
            let iso = super::iso::read(root, cc)?.ok_or_else(|| {
                format!(
                    "{}: no units, and no iso3166.yaml to take them from",
                    path.display()
                )
            })?;
            Split {
                level: f.level,
                units: iso.addresses(),
                source: iso.source,
                from_iso: true,
            }
        }
        _ => return Err(format!("{}: source and units go together", path.display())),
    };
    if s.source.trim().is_empty() || s.units.is_empty() {
        return Err(format!(
            "{}: a source and at least one unit",
            path.display()
        ));
    }
    let mut seen = BTreeSet::new();
    for code in s.units.values() {
        let ok = !code.is_empty()
            && code.len() <= 16
            && code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !code.starts_with('-')
            && !code.ends_with('-');
        if !ok || !seen.insert(code) {
            return Err(format!(
                "{}: code {code:?} is not a unique lower-case address part",
                path.display()
            ));
        }
    }
    Ok(s)
}

/// One part: its address, its own places and every place it holds, as indexes into the
/// build's places.
struct Part {
    address: String,
    /// The unit's place key.
    unit: String,
    own: BTreeSet<usize>,
    rows: BTreeSet<usize>,
}

/// The unit each place belongs to: the first place up its chain of parents (itself first)
/// that the split names. `None` for ancestors above the level, zones, and places with no unit.
fn units_of(places: &[Place], split: &Split) -> Vec<Option<usize>> {
    let index: BTreeMap<&str, usize> = places
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.as_str(), i))
        .collect();
    let units: BTreeMap<&str, usize> = split
        .units
        .keys()
        .filter_map(|k| index.get(k.as_str()).map(|&i| (k.as_str(), i)))
        .collect();
    places
        .iter()
        .map(|p| {
            let mut at = Some(p);
            let mut hops = 0;
            while let Some(q) = at {
                if let Some(&u) = units.get(q.key.as_str()) {
                    return Some(u);
                }
                hops += 1;
                if hops > 64 {
                    return None;
                }
                at = q
                    .parent
                    .as_deref()
                    .and_then(|k| index.get(k))
                    .and_then(|&i| places.get(i));
            }
            None
        })
        .collect()
}

/// The parts: own places by unit, then every ancestor of them, and how many places were given
/// to the nearest unit. A place under no unit that is neither above the units (the country) nor
/// a zone (a border town whose point is just outside every unit, a former region) goes to the
/// unit whose box of own places is nearest its point, ties to the first unit: a split never
/// drops a place.
fn parts(places: &[Place], split: &Split, cc: &str) -> Result<(Vec<Part>, usize), String> {
    let index: BTreeMap<&str, usize> = places
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.as_str(), i))
        .collect();
    for key in split.units.keys() {
        let p = index
            .get(key.as_str())
            .and_then(|&i| places.get(i))
            .ok_or_else(|| format!("split.yaml names {key}, which is not a place of this build"))?;
        if p.admin_level != split.level {
            return Err(format!(
                "split.yaml names {key} at level {}, not {}",
                p.admin_level, split.level
            ));
        }
    }
    let mut unit_of = units_of(places, split);
    let ancestors = |i: usize, into: &mut BTreeSet<usize>| {
        let mut at = places.get(i).and_then(|p| p.parent.as_deref());
        while let Some(k) = at {
            let Some(&j) = index.get(k) else { break };
            if !into.insert(j) {
                break;
            }
            at = places.get(j).and_then(|p| p.parent.as_deref());
        }
    };
    // Every place above the units: ancestors only.
    let mut above = BTreeSet::new();
    for key in split.units.keys() {
        if let Some(&u) = index.get(key.as_str()) {
            ancestors(u, &mut above);
        }
    }
    let outside: Vec<usize> = places
        .iter()
        .zip(&unit_of)
        .enumerate()
        .filter(|(i, (p, u))| {
            u.is_none() && p.kind != PlaceKind::TimezoneArea && !above.contains(i)
        })
        .map(|(i, _)| i)
        .collect();
    if !outside.is_empty() {
        let boxes = unit_boxes(places, &unit_of);
        for &i in &outside {
            let Some(p) = places.get(i) else { continue };
            let near = nearest(&boxes, p.point)
                .ok_or_else(|| format!("{}: no unit of the split has a place", p.key))?;
            if let Some(slot) = unit_of.get_mut(i) {
                *slot = Some(near);
            }
        }
    }
    let mut out = Vec::with_capacity(split.units.len());
    for (key, code) in &split.units {
        let Some(&u) = index.get(key.as_str()) else {
            continue;
        };
        let own: BTreeSet<usize> = unit_of
            .iter()
            .enumerate()
            .filter(|&(_, x)| *x == Some(u))
            .map(|(i, _)| i)
            .collect();
        let mut rows = own.clone();
        for &i in &own {
            ancestors(i, &mut rows);
        }
        out.push(Part {
            address: format!("{cc}-{code}"),
            unit: key.clone(),
            own,
            rows,
        });
    }
    Ok((out, outside.len()))
}

/// Each unit's box of own places, by the unit's index, in index order.
fn unit_boxes(places: &[Place], unit_of: &[Option<usize>]) -> Vec<(usize, Bbox)> {
    let mut boxes: BTreeMap<usize, Bbox> = BTreeMap::new();
    for (p, u) in places.iter().zip(unit_of) {
        let Some(u) = *u else { continue };
        let (x, y) = p.point;
        boxes
            .entry(u)
            .and_modify(|b| *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)))
            .or_insert((x, y, x, y));
    }
    boxes.into_iter().collect()
}

/// The unit whose box is nearest `point` (0 inside it), lon scaled by the cosine of the
/// latitude; the first such unit on a tie.
fn nearest(boxes: &[(usize, Bbox)], (x, y): (i32, i32)) -> Option<usize> {
    let scale = (f64::from(y) * 1e-7).to_radians().cos();
    let gap = |v: i32, lo: i32, hi: i32| f64::from((lo - v).max(0).max(v - hi));
    boxes
        .iter()
        .map(|&(u, (w, s, e, n))| {
            let dx = gap(x, w, e) * scale;
            let dy = gap(y, s, n);
            (dx.mul_add(dx, dy * dy), u)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, u)| u)
}

/// (west, south, east, north) on the 1e-7° grid.
type Bbox = (i32, i32, i32, i32);

/// The box of `points`.
fn bbox(points: impl Iterator<Item = (i32, i32)>) -> Option<Bbox> {
    points.fold(None, |b, (x, y)| {
        Some(b.map_or((x, y, x, y), |(w, s, e, n): Bbox| {
            (w.min(x), s.min(y), e.max(x), n.max(y))
        }))
    })
}

/// How far a part's zones reach past its box of own places: 0.05° (about 5 km), so no own
/// place lies on a cut.
const HALO: i32 = 500_000;

/// The zones cut to `area` grown by [`HALO`] ([`clip_ring`]), with their zone places; a zone
/// with nothing left is dropped. A zone keeps its point when the cut zone holds it, else takes
/// a point inside the cut zone. Inside the grown box every point has the area it has in the
/// country pack, so the part's own places keep theirs (proved by [`check`]); points beyond it
/// have none in the part.
fn zones_in(zones: &Zones, area: Bbox) -> Result<Zones, String> {
    let (west, south, east, north) = area;
    let grown = (
        west.saturating_sub(HALO),
        south.saturating_sub(HALO),
        east.saturating_add(HALO),
        north.saturating_add(HALO),
    );
    let mut out = Zones {
        places: Vec::new(),
        rings: Vec::new(),
    };
    for (place, rings) in zones.places.iter().zip(&zones.rings) {
        let cut: Vec<Vec<(i32, i32)>> = rings
            .iter()
            .map(|r| {
                let r: Vec<(i32, i32)> = r.iter().map(|&[x, y]| (x, y)).collect();
                clip_ring(&r, grown)
            })
            .filter(|r| !r.is_empty())
            .collect();
        if cut.is_empty() {
            continue;
        }
        let poly = Polygon { rings: cut };
        let mut place = place.clone();
        if !poly.contains(place.point.0, place.point.1) {
            place.point = poly
                .interior_point()
                .ok_or_else(|| format!("{}: no point inside the cut zone", place.key))?;
        }
        out.places.push(place);
        out.rings.push(
            poly.rings
                .into_iter()
                .map(|r| r.into_iter().map(|(x, y)| [x, y]).collect())
                .collect(),
        );
    }
    Ok(out)
}

/// Removes the parts of build `build` of `cc` in `out_dir` (`<cc>-<code>-places-<build>.tgd`),
/// so a build never leaves another build's parts beside its country pack.
pub(crate) fn remove_parts(out_dir: &Path, cc: &str, build: &str) -> Result<(), String> {
    let Ok(dir) = std::fs::read_dir(out_dir) else {
        return Ok(());
    };
    let (head, tail) = (format!("{cc}-"), format!("-places-{build}.tgd"));
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let code = name.strip_prefix(&head).and_then(|n| n.strip_suffix(&tail));
        if code.is_some_and(|c| !c.is_empty() && !c.contains("-places")) {
            std::fs::remove_file(entry.path())
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// A written part: its address, its own places' keys, and its bytes.
type Written = (String, BTreeSet<String>, Built);

/// Writes every part of `split` beside the country pack `whole` (written from `input`), then
/// proves them against it with [`check`]. Returns the report.
pub(crate) fn run(
    input: &Input<'_>,
    whole: &Built,
    split: &Split,
    levels: &[Level],
    out_dir: &Path,
) -> Result<String, String> {
    let cc = input.args.cc.as_str();
    let places = input.places;
    let split = &split.for_places(places);
    let mut report = format!(
        "\n## Split (D-191)\n\nLevel {}, {} parts, codes from {}.\n\n| Part | Own places | Rows | Zones | Bytes |\n|---|---:|---:|---:|---:|\n",
        split.level,
        split.units.len(),
        split.source
    );
    let (all, near) = parts(places, split, cc)?;
    let mut written = Vec::new();
    for part in all {
        // The part's area: its unit's polygon at the split's level (D-192).
        let outline: Vec<&Polygon> = levels
            .iter()
            .filter(|l| l.number == split.level)
            .flat_map(|l| l.keys.iter().zip(&l.polys))
            .filter(|(k, _)| **k == part.unit)
            .map(|(_, p)| p)
            .collect();
        let (line, done) = write_part(input, whole, &part, &outline, out_dir)?;
        report.push_str(&line);
        written.push(done);
    }
    let line = check(&whole.bytes, &whole.given, &written)?;
    let _ = writeln!(
        report,
        "\n{near} places under no unit (points just outside every unit, former units) went to the nearest unit by its box of places.\n\n{line}"
    );
    Ok(report)
}

/// Writes one part through the pack stage; its report row, and what [`check`] needs.
fn write_part(
    input: &Input<'_>,
    whole: &Built,
    part: &Part,
    outline: &[&Polygon],
    out_dir: &Path,
) -> Result<(String, Written), String> {
    let places = input.places;
    let rows = &part.rows;
    // The part's box: its own places, and its own units' outlines when the build has them.
    let area = bbox(
        part.own
            .iter()
            .filter_map(|&i| places.get(i))
            .map(|p| p.point),
    )
    .ok_or_else(|| format!("{}: no places", part.address))?;
    let zones = zones_in(input.zones, area)?;
    // Then the cut zones' places (a point may have moved into its cut zone).
    let mut at: Vec<usize> = rows.iter().copied().collect();
    let mut part_places: Vec<Place> = at.iter().filter_map(|&i| places.get(i).cloned()).collect();
    for z in &zones.places {
        if let Some(i) = places.iter().position(|p| p.key == z.key) {
            at.push(i);
            part_places.push(z.clone());
        }
    }
    let keys: BTreeSet<&str> = part_places.iter().map(|p| p.key.as_str()).collect();
    let own_keys: BTreeSet<&str> = part
        .own
        .iter()
        .filter_map(|&i| places.get(i))
        .map(|p| p.key.as_str())
        .collect();
    // Outlines of its own places only: a stub's outline stays with the country pack and the
    // embedded baseline (D-144, D-145).
    let country = places
        .iter()
        .find(|p| p.kind == PlaceKind::Country)
        .map(|p| p.key.as_str());
    let geometry = input
        .geometry
        .map(|g| g.subset(country, |k| own_keys.contains(k)))
        .transpose()?;
    let post = input.postal.map(|p| postal::Postal {
        rules: p.rules.clone(),
        entries: p
            .entries
            .iter()
            .filter(|e| own_keys.contains(e.unit.as_str()))
            .cloned()
            .collect(),
        notes: Vec::new(),
    });
    let heights = input.elevation.map(|e| e.subset(&at));
    let own_ids: BTreeSet<_> = own_keys
        .iter()
        .filter_map(|k| whole.given.ids.get(*k))
        .collect();
    let given = pack::Given {
        ids: whole
            .given
            .ids
            .iter()
            .filter(|(k, _)| keys.contains(k.as_str()))
            .map(|(k, &v)| (k.clone(), v))
            .collect(),
        moved: whole
            .given
            .moved
            .iter()
            .filter(|(_, to)| to.as_ref().is_none_or(|t| own_ids.contains(t)))
            .copied()
            .collect(),
    };
    let file = out_dir.join(format!(
        "{}-places-{}.tgd",
        part.address,
        input.args.build_name()
    ));
    let part_input = Input {
        root: input.root,
        args: input.args,
        address: &part.address,
        given: Some(&given),
        places: &part_places,
        profile: input.profile,
        zones: &zones,
        coverage: area,
        file: &file,
        registry: input.registry,
        ids_file: None,
        geometry: geometry.as_ref(),
        postal: post.as_ref().filter(|p| !p.entries.is_empty()),
        elevation: heights.as_ref(),
        outline,
        attribution: input.attribution,
        stated_tz: input.stated_tz,
        iso: input.iso,
    };
    let (_, built) = pack::write(&part_input)?;
    let line = format!(
        "| `{}` | {} | {} | {} | {} |\n",
        part.address,
        part.own.len(),
        part_places.len(),
        zones.places.len(),
        built.bytes.len()
    );
    let own = own_keys.into_iter().map(str::to_owned).collect();
    Ok((line, (part.address.clone(), own, built)))
}

/// Each part against the country pack (D-191): every own place has the same ID, point,
/// canonical name and chain of ancestors by ID as in the country pack, and every place of the
/// country pack is owned by at most one part. Returns the summary line.
pub(crate) fn check(
    whole: &[u8],
    given: &pack::Given,
    parts: &[Written],
) -> Result<String, String> {
    let w = Opened::new(whole)?;
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    let mut checked = 0_usize;
    let mut coded = 0_usize;
    let mut opened = Vec::with_capacity(parts.len());
    for (address, own, built) in parts {
        let p = Opened::new(&built.bytes)?;
        for key in own {
            if let Some(other) = owner.insert(key, address) {
                return Err(format!("{key}: owned by {other} and {address}"));
            }
            let id = *given
                .ids
                .get(key)
                .ok_or_else(|| format!("{key}: no ID in the country pack"))?;
            let wr = w
                .ids
                .row(id)
                .ok_or_else(|| format!("{id}: not in the country pack"))?;
            let pr = p
                .ids
                .row(id)
                .ok_or_else(|| format!("{address}: {id} ({key}) is missing"))?;
            if w.places.point(wr) != p.places.point(pr) {
                return Err(format!("{address}: {id} moved"));
            }
            if canonical(&w, wr)? != canonical(&p, pr)? {
                return Err(format!("{address}: {id} is named differently"));
            }
            let chain = |o: &Opened<'_>, row: usize| -> Vec<_> {
                o.hier.ancestors(row).filter_map(|r| o.ids.id(r)).collect()
            };
            if chain(&w, wr) != chain(&p, pr) {
                return Err(format!(
                    "{address}: {id} ({key}) has another chain of ancestors (a stub is missing)"
                ));
            }
            if w.iso(wr) != p.iso(pr) {
                return Err(format!("{address}: {id} ({key}) has another ISO 3166 code"));
            }
            if w.zone(wr) != p.zone(pr) {
                return Err(format!(
                    "{address}: {id} ({key}) has another timezone area (a zone is missing)"
                ));
            }
            checked += 1;
        }
        // Stubs carry their codes too: every row of the part has the country pack's code.
        for pr in 0..p.places.len() {
            let id = p
                .ids
                .id(pr)
                .ok_or_else(|| format!("{address}: row {pr} has no ID"))?;
            let wr = w
                .ids
                .row(id)
                .ok_or_else(|| format!("{address}: {id} is not in the country pack"))?;
            if w.iso(wr) != p.iso(pr) {
                return Err(format!("{address}: {id} has another ISO 3166 code"));
            }
            coded += usize::from(p.iso(pr).is_some());
        }
        opened.push(p);
    }
    // No place is dropped: each row of the country pack is in some part, as an own place, a
    // stub or a zone; only a zone that holds no place (a neighbour's, cut away) may be in none.
    let used: BTreeSet<_> = (0..w.places.len())
        .filter(|&r| w.places.kind(r) != Some(PlaceKind::TimezoneArea))
        .filter_map(|r| w.zone(r))
        .collect();
    let mut unused = 0_usize;
    for row in 0..w.places.len() {
        let id = w.ids.id(row).ok_or_else(|| format!("row {row}: no ID"))?;
        if !opened.iter().any(|p| p.ids.row(id).is_some()) {
            if w.places.kind(row) == Some(PlaceKind::TimezoneArea) && !used.contains(&id) {
                unused += 1;
                continue;
            }
            return Err(format!("{id}: in the country pack and in no part"));
        }
    }
    Ok(format!(
        "Split checked against the country pack: {checked} own places in {} parts keep their ID, point, canonical name, chain of ancestors, timezone area and ISO 3166 code; {coded} coded rows (stubs included) carry the country pack's code; none is in two parts; all {} rows of the country pack are in a part but {unused} timezone areas that hold no place.",
        parts.len(),
        w.places.len() - unused
    ))
}

/// The canonical name of `row`, as text.
fn canonical(o: &Opened<'_>, row: usize) -> Result<String, String> {
    let mut buf = vec![0_u8; teigeodata_format::names::TEXT_BUFFER];
    o.names
        .canonical(row)
        .map(|n| n.text(&mut buf).map(str::to_owned))
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|e| e.to_string())
}

/// The sections [`check`] reads.
struct Opened<'a> {
    ids: IdMap<'a>,
    places: Places<'a>,
    names: Names<'a>,
    hier: Hierarchy<'a>,
    tz: Option<PlaceTz<'a>>,
    iso: Option<IsoCodes<'a>>,
}

impl<'a> Opened<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, String> {
        let e = |e: teigeodata_format::Error| e.to_string();
        let p = Pack::open(bytes).map_err(e)?;
        let s = |k| p.section(k).ok_or(format!("no {k}"));
        let places = Places::open(&s(Kind::PLACES)?).map_err(e)?;
        Ok(Self {
            ids: IdMap::open(&s(Kind::IDMAP)?).map_err(e)?,
            iso: p
                .section(Kind::ISO_CODES)
                .map(|t| IsoCodes::open(&t, places.len()))
                .transpose()
                .map_err(e)?,
            places,
            names: Names::open(&s(Kind::NAMES)?).map_err(e)?,
            hier: Hierarchy::open(&s(Kind::HIERARCHY)?).map_err(e)?,
            tz: p
                .section(Kind::PLACE_TZ)
                .map(|t| PlaceTz::open(&t))
                .transpose()
                .map_err(e)?,
        })
    }

    /// The ISO 3166 code of `row`, if it has one.
    fn iso(&self, row: usize) -> Option<teigeodata_core::IsoCode> {
        self.iso.as_ref()?.get(u32::try_from(row).ok()?)
    }

    /// The ID of `row`'s timezone area, if it has one.
    fn zone(&self, row: usize) -> Option<teigeodata_core::PlaceId> {
        let at = self.tz.as_ref()?.get(row)?;
        self.ids.id(usize::try_from(at).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use teigeodata_core::PlaceKind;

    use super::{Split, Zones, bbox, parts, read, remove_parts, zones_in};
    use crate::build::Place;

    fn place(key: &str, kind: PlaceKind, level: u8, parent: Option<&str>, x: i32) -> Place {
        Place {
            key: key.to_owned(),
            kind,
            admin_level: level,
            flags: 0,
            point: (x, x),
            names: Vec::new(),
            population: None,
            rank: 0,
            parent: parent.map(str::to_owned),
            sources: vec![key.to_owned()],
        }
    }

    fn split(units: &[(&str, &str)]) -> Split {
        Split {
            level: 1,
            source: "test".to_owned(),
            units: units
                .iter()
                .map(|&(k, c)| (k.to_owned(), c.to_owned()))
                .collect::<BTreeMap<_, _>>(),
            from_iso: false,
        }
    }

    /// A country, two provinces, a district in each, a village in one, and a zone.
    fn country() -> Vec<Place> {
        vec![
            place("c", PlaceKind::Country, 0, None, 0),
            place("a", PlaceKind::Admin, 1, Some("c"), 10),
            place("b", PlaceKind::Admin, 1, Some("c"), 20),
            place("a1", PlaceKind::Admin, 2, Some("a"), 11),
            place("b1", PlaceKind::Admin, 2, Some("b"), 21),
            place("v", PlaceKind::Locality, 0, Some("a1"), 12),
            place("tz", PlaceKind::TimezoneArea, 0, None, 0),
        ]
    }

    #[test]
    fn every_place_is_owned_once_and_carries_its_ancestors() {
        let places = country();
        let (got, near) = parts(&places, &split(&[("a", "p1"), ("b", "p2")]), "xx").unwrap();
        assert_eq!(near, 0);
        let keys = |s: &std::collections::BTreeSet<usize>| -> Vec<&str> {
            s.iter().map(|&i| places[i].key.as_str()).collect()
        };
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].address, "xx-p1");
        assert_eq!(keys(&got[0].own), ["a", "a1", "v"]);
        assert_eq!(keys(&got[0].rows), ["c", "a", "a1", "v"]);
        assert_eq!(got[1].address, "xx-p2");
        assert_eq!(keys(&got[1].own), ["b", "b1"]);
        assert_eq!(keys(&got[1].rows), ["c", "b", "b1"]);
    }

    #[test]
    fn a_place_under_no_unit_goes_to_the_nearest_unit() {
        let mut places = country();
        // A border town parented to the country, nearer province b's places (20, 21).
        places.push(place("edge", PlaceKind::Settlement, 0, Some("c"), 24));
        let (got, near) = parts(&places, &split(&[("a", "p1"), ("b", "p2")]), "xx").unwrap();
        assert_eq!(near, 1);
        let edge = places.len() - 1;
        assert!(got[1].own.contains(&edge));
        assert!(!got[0].own.contains(&edge));
        assert!(
            got[1].rows.contains(&0),
            "its parent, the country, is a stub"
        );
    }

    #[test]
    fn a_unit_must_be_a_place_at_the_level() {
        let places = country();
        let e = parts(&places, &split(&[("a1", "p1")]), "xx").err().unwrap();
        assert!(e.contains("level 2, not 1"), "{e}");
        let e = parts(&places, &split(&[("nope", "p1")]), "xx")
            .err()
            .unwrap();
        assert!(e.contains("not a place of this build"), "{e}");
    }

    #[test]
    fn zones_are_cut_to_the_part() {
        const D: i32 = 10_000_000; // 1°
        let zones = Zones {
            places: vec![
                place("tz:in", PlaceKind::TimezoneArea, 0, None, 9 * D),
                place("tz:out", PlaceKind::TimezoneArea, 0, None, 55 * D),
            ],
            rings: vec![
                vec![vec![
                    [0, 0],
                    [10 * D, 0],
                    [10 * D, 10 * D],
                    [0, 10 * D],
                    [0, 0],
                ]],
                vec![vec![
                    [50 * D, 50 * D],
                    [60 * D, 50 * D],
                    [60 * D, 60 * D],
                    [50 * D, 50 * D],
                ]],
            ],
        };
        let area = bbox([(D, D), (2 * D, 2 * D)].into_iter()).unwrap();
        let kept = zones_in(&zones, area).unwrap();
        assert_eq!(kept.places.len(), 1);
        assert_eq!(kept.places[0].key, "tz:in");
        let ring = &kept.rings[0][0];
        let (lo, hi) = (D - super::HALO, 2 * D + super::HALO);
        assert!(
            ring.iter()
                .all(|&[x, y]| (lo..=hi).contains(&x) && (lo..=hi).contains(&y))
        );
        // Its point (9°, 9°) was cut off: it moved inside the cut zone.
        let (x, y) = kept.places[0].point;
        assert!((lo..=hi).contains(&x) && (lo..=hi).contains(&y), "{x}, {y}");
    }

    #[test]
    fn split_files_are_checked() {
        let dir = std::env::temp_dir().join(format!("forge-split-{}", std::process::id()));
        let cc = dir.join("data/countries/kq");
        std::fs::create_dir_all(&cc).unwrap();
        let write = |text: &str| std::fs::write(cc.join("split.yaml"), text).unwrap();
        let head = "schema: teigeodata/split/v1\nlevel: 1\nsource: test\n";
        write(&format!("{head}units:\n  a: p1\n  b: p2\n"));
        assert_eq!(read(&dir, "kq").unwrap().units.len(), 2);
        for bad in [
            format!("{head}units:\n  a: p1\n  b: p1\n"),
            format!("{head}units:\n  a: P1\n"),
            format!("{head}units:\n  a: -p\n"),
            format!("{head}units: {{}}\n"),
            "schema: other\nlevel: 1\nsource: test\nunits:\n  a: p1\n".to_owned(),
            format!("{head}units:\n  a: p1\nextra: 1\n"),
            "schema: teigeodata/split/v1\nlevel: 1\nsource: test\n".to_owned(),
            "schema: teigeodata/split/v1\nlevel: 1\n".to_owned(),
        ] {
            write(&bad);
            assert!(read(&dir, "kq").is_err(), "{bad}");
        }
        // Without units of its own, the units are iso3166.yaml's, of its level only.
        write("schema: teigeodata/split/v1\nlevel: 1\n");
        std::fs::write(
            cc.join("iso3166.yaml"),
            "schema: teigeodata/iso3166/v1\ncountry: KQ\nsource: test\nunits:\n  a: KQ-P1\n  b: KQ-P2\n  c: KQ-1A\n",
        )
        .unwrap();
        let s = read(&dir, "kq").unwrap();
        assert_eq!(s.units.len(), 3);
        assert_eq!(s.units.get("a").map(String::as_str), Some("p1"));
        let places = [
            place("a", PlaceKind::Admin, 1, None, 0),
            place("b", PlaceKind::Admin, 1, None, 0),
            place("c", PlaceKind::Admin, 2, Some("a"), 0),
        ];
        let at = s.for_places(&places);
        assert_eq!(at.units.keys().collect::<Vec<_>>(), ["a", "b"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_builds_own_parts_are_removed() {
        let dir = std::env::temp_dir().join(format!("forge-split-rm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let names = [
            "xx-p1-places-main.tgd",
            "xx-p2-places-main.tgd",
            "xx-places-main.tgd",
            "xx-p1-places-lite.tgd",
            "yy-p1-places-main.tgd",
            "xx-ids-main.tsv",
        ];
        for n in names {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        remove_parts(&dir, "xx", "main").unwrap();
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "xx-ids-main.tsv",
                "xx-p1-places-lite.tgd",
                "xx-places-main.tgd",
                "yy-p1-places-main.tgd"
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
