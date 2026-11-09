//! Corrections as reviewed data with guards (ADR-0036, D-117).
//!
//! One YAML file per correction, `data/countries/<cc>/corrections/<id>.yaml` (schema
//! `teigeodata/correction/v1`). A correction targets the place that holds a source record and
//! carries a guard: a source record, a field of it, and the BLAKE3 of that field's canonical
//! text when the correction was written. Every build gives each correction one outcome:
//!
//! - **retire** when the sources already say what the correction says (fixed upstream);
//! - **apply** when the guard still holds;
//! - **stale** otherwise (the guard's value changed, or its record or target is gone): not
//!   applied, listed with the value it now has.
//!
//! A stale correction created before the previous release's month fails the build (G-corr).
//! The lite build skips a correction whose evidence is not all lite-clean (`CLEAN_ROOM.md`).
//! Canonical text of a guarded field: `name` is the record's name; `point` is
//! `<lat_e7>,<lon_e7>`; `population` is the number, or empty when there is none.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;

use super::sources::{Rec, read_records};
use super::units::{Level, deepest};
use super::{LITE, Place, PlaceName};

/// `review.by` of a correction no person has reviewed yet: applied, and counted in the summary
/// until someone reviews it.
pub(super) const UNREVIEWED: &str = "unreviewed";

/// The schema every correction file names.
pub(super) const SCHEMA: &str = "teigeodata/correction/v1";

/// A correction file.
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "Raw")]
pub(super) struct Correction {
    pub(super) schema: String,
    pub(super) id: String,
    /// A source record (`geonames:1283240`): the correction applies to the place holding it.
    pub(super) target: String,
    pub(super) op: Op,
    pub(super) guard: Guard,
    pub(super) evidence: Vec<Evidence>,
    /// `YYYY-MM-DD`.
    pub(super) created: String,
    pub(super) author: String,
    pub(super) review: Review,
}

/// A correction file as written: `op` names the operation and its own fields sit beside it.
/// Every field is checked, so a misspelt or misplaced one is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    schema: String,
    id: String,
    target: String,
    op: String,
    name: Option<NewName>,
    text: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    population: Option<i64>,
    other: Option<String>,
    guard: Guard,
    evidence: Vec<Evidence>,
    created: String,
    author: String,
    review: Review,
}

impl TryFrom<Raw> for Correction {
    type Error = String;

    fn try_from(r: Raw) -> Result<Self, String> {
        let given = [
            ("name", r.name.is_some()),
            ("text", r.text.is_some()),
            ("lat", r.lat.is_some()),
            ("lon", r.lon.is_some()),
            ("population", r.population.is_some()),
            ("other", r.other.is_some()),
        ];
        let (op, uses): (Option<Op>, &[&str]) = match r.op.as_str() {
            "add-name" => (r.name.map(|name| Op::AddName { name }), &["name"]),
            "remove-name" => (r.text.map(|text| Op::RemoveName { text }), &["text"]),
            "set-point" => (
                r.lat.zip(r.lon).map(|(lat, lon)| Op::SetPoint { lat, lon }),
                &["lat", "lon"],
            ),
            "set-population" => (
                r.population
                    .map(|population| Op::SetPopulation { population }),
                &["population"],
            ),
            "same-as" => (r.other.map(|other| Op::SameAs { other }), &["other"]),
            o => return Err(format!("op {o} is unknown")),
        };
        if let Some((f, _)) = given.iter().find(|(f, set)| *set && !uses.contains(f)) {
            return Err(format!("op {} takes no {f}", r.op));
        }
        let op = op.ok_or_else(|| format!("op {} needs {}", r.op, uses.join(" and ")))?;
        Ok(Self {
            schema: r.schema,
            id: r.id,
            target: r.target,
            op,
            guard: r.guard,
            evidence: r.evidence,
            created: r.created,
            author: r.author,
            review: r.review,
        })
    }
}

/// What a correction does.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Op {
    /// Adds a name the sources lack.
    AddName { name: NewName },
    /// Removes a name (never the canonical one).
    RemoveName { text: String },
    /// Moves the place.
    SetPoint { lat: f64, lon: f64 },
    /// Sets the population.
    SetPopulation { population: i64 },
    /// The place holding `other` is the same place: it is merged into the target's.
    SameAs { other: String },
}

/// A name to add.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct NewName {
    pub(super) text: String,
    #[serde(default)]
    pub(super) lang: Option<String>,
    /// `official`, `local`, `alt`, `colloquial`, `historical` or `former-official`.
    #[serde(rename = "type")]
    pub(super) kind: String,
}

/// The source value a correction was written against.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard {
    pub(super) record: String,
    /// `name`, `point` or `population`.
    pub(super) field: String,
    /// BLAKE3 (hex) of the field's canonical text.
    pub(super) value_hash: String,
}

/// One piece of evidence.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    pub(super) source: String,
    #[serde(default)]
    pub(super) url: Option<String>,
    pub(super) licence_class: String,
}

/// Who reviewed it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Review {
    pub(super) by: String,
    pub(super) on: String,
}

/// What a build did with a correction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Applied,
    Retired,
    /// Why, with the value the guard field has now.
    Stale(String),
    /// Evidence that the lite build may not use.
    NotLite,
}

/// The name type of a correction's `type`.
pub(super) fn name_type(kind: &str) -> Option<NameType> {
    Some(match kind {
        "official" => NameType::Official,
        "local" => NameType::Local,
        "alt" => NameType::Alt,
        "colloquial" => NameType::Colloquial,
        "historical" => NameType::Historical,
        "former-official" => NameType::FormerOfficial,
        _ => return None,
    })
}

/// The canonical text of a record's field, or `None` for an unknown field.
pub(super) fn canonical(rec: &Rec, field: &str) -> Option<String> {
    Some(match field {
        "name" => rec.name.clone(),
        "point" => rec
            .point
            .map(|(lon, lat)| format!("{lat},{lon}"))
            .unwrap_or_default(),
        "population" => rec.population.map(|p| p.to_string()).unwrap_or_default(),
        _ => return None,
    })
}

/// The hash a guard stores for a value.
pub(super) fn hash(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}

/// Reads and checks every correction of a country, sorted by id.
pub(super) fn load(root: &Path, cc: &str) -> Result<Vec<Correction>, String> {
    let dir = root.join("data/countries").join(cc).join("corrections");
    let Ok(rd) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<_> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    files.sort();
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let text = fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let c: Correction =
            serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        validate(&c, stem).map_err(|e| format!("{}: {e}", f.display()))?;
        out.push(c);
    }
    Ok(out)
}

/// The checks `load` makes on one file (also run by `cargo xtask check-data`'s twin).
pub(super) fn validate(c: &Correction, stem: &str) -> Result<(), String> {
    if c.schema != SCHEMA {
        return Err(format!("schema {} is not {SCHEMA}", c.schema));
    }
    if c.id != stem {
        return Err(format!("id {} is not the file name {stem}", c.id));
    }
    for r in [&c.target, &c.guard.record] {
        if !r.contains(':') {
            return Err(format!("{r} is not a source record (<source>:<id>)"));
        }
    }
    if !matches!(c.guard.field.as_str(), "name" | "point" | "population") {
        return Err(format!("guard field {} is unknown", c.guard.field));
    }
    if c.guard.value_hash.len() != 64 || !c.guard.value_hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("guard value_hash is not 64 hex digits".into());
    }
    if c.evidence.is_empty() {
        return Err("no evidence".into());
    }
    for e in &c.evidence {
        if e.source.trim().is_empty() || e.licence_class.trim().is_empty() {
            return Err("evidence needs a source and a licence class".into());
        }
        if e.url.as_deref().is_some_and(|u| !u.starts_with("https://")) {
            return Err("evidence URLs are https".into());
        }
    }
    if c.author.trim().is_empty() || c.review.by.trim().is_empty() {
        return Err("author and reviewer are required".into());
    }
    if !is_date(&c.created) || !is_date(&c.review.on) {
        return Err("dates are YYYY-MM-DD".into());
    }
    match &c.op {
        Op::AddName { name } if name_type(&name.kind).is_none() => {
            Err(format!("name type {} is unknown", name.kind))
        }
        Op::AddName { name } if name.text.trim().is_empty() => Err("an empty name".into()),
        Op::SetPoint { lat, lon }
            if !(lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0) =>
        {
            Err("point out of range".into())
        }
        Op::SetPopulation { population } if *population < 0 => Err("negative population".into()),
        _ => Ok(()),
    }
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// The place index holding a source record.
fn holding(places: &[Place], record: &str) -> Option<usize> {
    places
        .iter()
        .position(|p| p.key == record || p.sources.iter().any(|s| s == record))
}

/// (lon, lat) on the 1e-7° grid.
#[allow(clippy::cast_possible_truncation, reason = "checked to ±180°")]
fn e7(lat: f64, lon: f64) -> (i32, i32) {
    ((lon * 1e7).round() as i32, (lat * 1e7).round() as i32)
}

/// Whether the sources already say what the correction says.
fn upstream_agrees(c: &Correction, places: &[Place], at: usize, guard: &Rec) -> bool {
    let Some(p) = places.get(at) else {
        return false;
    };
    match &c.op {
        Op::AddName { name } => p.names.iter().any(|n| n.text == name.text),
        Op::RemoveName { text } => !p.names.iter().any(|n| &n.text == text),
        Op::SetPoint { lat, lon } => guard.point == Some(e7(*lat, *lon)),
        Op::SetPopulation { population } => guard.population == Some(*population),
        Op::SameAs { other } => holding(places, other) == Some(at),
    }
}

/// Applies every correction to `places`; returns each one's outcome, by id.
pub(super) fn apply(
    corrections: &[Correction],
    places: &mut [Place],
    records: &BTreeMap<String, Rec>,
    levels: &[Level],
    lite: bool,
) -> Result<Vec<(String, Outcome)>, String> {
    let mut out = Vec::with_capacity(corrections.len());
    for c in corrections
        .iter()
        .filter(|c| !matches!(c.op, Op::SameAs { .. }))
    {
        let outcome = one(c, places, records, levels, lite)?;
        out.push((c.id.clone(), outcome));
    }
    Ok(out)
}

fn one(
    c: &Correction,
    places: &mut [Place],
    records: &BTreeMap<String, Rec>,
    levels: &[Level],
    lite: bool,
) -> Result<Outcome, String> {
    if lite
        && !c
            .evidence
            .iter()
            .all(|e| LITE.contains(&e.licence_class.as_str()))
    {
        return Ok(Outcome::NotLite);
    }
    let Some(guard) = records.get(&c.guard.record) else {
        return Ok(Outcome::Stale(format!(
            "guard record {} is gone",
            c.guard.record
        )));
    };
    let Some(at) = holding(places, &c.target) else {
        return Ok(Outcome::Stale(format!(
            "target {} is in no place",
            c.target
        )));
    };
    if upstream_agrees(c, places, at, guard) {
        return Ok(Outcome::Retired);
    }
    let now = canonical(guard, &c.guard.field).unwrap_or_default();
    if hash(&now) != c.guard.value_hash {
        return Ok(Outcome::Stale(format!("{} is now {now:?}", c.guard.field)));
    }
    match &c.op {
        Op::AddName { name } => {
            if let Some(p) = places.get_mut(at) {
                p.names.push(PlaceName {
                    text: name.text.clone(),
                    lang: name.lang.clone(),
                    kind: name_type(&name.kind),
                });
            }
        }
        Op::RemoveName { text } => {
            let Some(p) = places.get_mut(at) else {
                return Ok(Outcome::Stale("target gone".into()));
            };
            if p.names.first().is_some_and(|n| &n.text == text) {
                return Err(format!("{}: would remove the canonical name", c.id));
            }
            p.names.retain(|n| &n.text != text);
        }
        Op::SetPoint { lat, lon } => {
            if let Some(p) = places.get_mut(at) {
                p.point = e7(*lat, *lon);
                // A settlement's parent is the deepest unit holding it; a unit keeps its own.
                if !matches!(p.kind, PlaceKind::Admin | PlaceKind::Country)
                    && let Some(k) = deepest(levels, p.point)
                {
                    p.parent = Some(k);
                }
            }
        }
        Op::SetPopulation { population } => {
            if let Some(p) = places.get_mut(at) {
                p.population = Some(*population);
            }
        }
        // Applied to the clusters, before naming (`link_clusters`).
        Op::SameAs { .. } => {}
    }
    Ok(Outcome::Applied)
}

/// The clusters of the conflation, by cluster key: each member's source index and record.
pub(super) type Clusters = BTreeMap<String, Vec<(usize, String)>>;

/// Applies the `same-as` corrections to the clusters, before places are named: the cluster
/// holding `other` joins the target's, so naming, the alias rules and the core see one place
/// (ADR-0036: a must-link). Returns each one's outcome.
pub(super) fn link_clusters(
    corrections: &[Correction],
    clusters: &mut Clusters,
    records: &[BTreeMap<String, Rec>],
    lite: bool,
) -> Vec<(String, Outcome)> {
    let find = |clusters: &Clusters, record: &str| {
        clusters
            .iter()
            .find(|(_, m)| m.iter().any(|(_, id)| id == record))
            .map(|(k, _)| k.clone())
    };
    let mut out = Vec::new();
    for c in corrections {
        let Op::SameAs { other } = &c.op else {
            continue;
        };
        let outcome = (|| {
            if lite
                && !c
                    .evidence
                    .iter()
                    .all(|e| LITE.contains(&e.licence_class.as_str()))
            {
                return Outcome::NotLite;
            }
            let Some(guard) = records.iter().find_map(|r| r.get(&c.guard.record)) else {
                return Outcome::Stale(format!("guard record {} is gone", c.guard.record));
            };
            let (Some(at), Some(o)) = (find(clusters, &c.target), find(clusters, other)) else {
                return Outcome::Stale(format!("{} or {other} is in no cluster", c.target));
            };
            if at == o {
                return Outcome::Retired;
            }
            let now = canonical(guard, &c.guard.field).unwrap_or_default();
            if hash(&now) != c.guard.value_hash {
                return Outcome::Stale(format!("{} is now {now:?}", c.guard.field));
            }
            let moved = clusters.remove(&o).unwrap_or_default();
            if let Some(m) = clusters.get_mut(&at) {
                m.extend(moved);
                // Records stay in source priority; within a source, in their order.
                m.sort_by_key(|(src, _)| *src);
            }
            Outcome::Applied
        })();
        out.push((c.id.clone(), outcome));
    }
    out
}

/// The first day of the month before a release's (`2026.11.0` → `2026-10-01`): a stale
/// correction created before it has stayed stale through a release.
pub(super) fn previous_release_month(release: &str) -> Option<String> {
    let mut it = release.split('.');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) {
        return None;
    }
    let (y, m) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    Some(format!("{y:04}-{m:02}-01"))
}

/// G-corr: the stale corrections older than the previous release, which fail the build.
pub(super) fn overdue<'c>(
    corrections: &'c [Correction],
    outcomes: &[(String, Outcome)],
    release: &str,
) -> Vec<&'c str> {
    let Some(cut) = previous_release_month(release) else {
        return Vec::new();
    };
    let stale = |id: &str| {
        outcomes
            .iter()
            .any(|(o_id, o)| o_id == id && matches!(o, Outcome::Stale(_)))
    };
    corrections
        .iter()
        .filter(|c| c.created < cut && stale(&c.id))
        .map(|c| c.id.as_str())
        .collect()
}

/// Writes `corrections-<build>.tsv` and returns the summary line.
pub(super) fn report(
    outcomes: &[(String, Outcome)],
    out_dir: &Path,
    build: &str,
) -> Result<String, String> {
    let mut tsv = String::from(
        "# Corrections and what this build did with them (ADR-0036).\nid\toutcome\twhy\n",
    );
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (id, o) in outcomes {
        let (name, why) = match o {
            Outcome::Applied => ("applied", ""),
            Outcome::Retired => ("retired", "the sources now agree"),
            Outcome::Stale(w) => ("stale", w.as_str()),
            Outcome::NotLite => ("not in lite", "evidence not lite-clean"),
        };
        *counts.entry(name).or_default() += 1;
        let _ = writeln!(tsv, "{id}\t{name}\t{why}");
    }
    let file = out_dir.join(format!("corrections-{build}.tsv"));
    fs::write(&file, tsv).map_err(|e| format!("{}: {e}", file.display()))?;
    let n = |k: &str| counts.get(k).copied().unwrap_or(0);
    Ok(format!(
        "Corrections (ADR-0036): {} applied, {} retired, {} stale, {} not in lite; listed in `corrections-{build}.tsv`.\n",
        n("applied"),
        n("retired"),
        n("stale"),
        n("not in lite")
    ))
}

/// Loads the country's corrections, applies them to `places` and enforces G-corr; returns the
/// summary line (empty when the country has none).
pub(super) fn run(
    root: &Path,
    (corrections, linked, release, lite): (&[Correction], &[(String, Outcome)], &str, bool),
    sources: &[String],
    places: &mut [Place],
    levels: &[Level],
    out_dir: &Path,
) -> Result<String, String> {
    if corrections.is_empty() {
        return Ok(String::new());
    }
    let build = root.join("data/build");
    let mut records = BTreeMap::new();
    for s in sources {
        records.append(&mut read_records(&build, s)?);
    }
    let mut outcomes = apply(corrections, places, &records, levels, lite)?;
    outcomes.extend(linked.iter().cloned());
    outcomes.sort_by(|a, b| a.0.cmp(&b.0));

    let past_due = overdue(corrections, &outcomes, release);
    if !past_due.is_empty() {
        return Err(format!(
            "G-corr: stale corrections older than the previous release: {}",
            past_due.join(", ")
        ));
    }
    let mut summary = report(&outcomes, out_dir, if lite { "lite" } else { "main" })?;
    let unreviewed = corrections
        .iter()
        .filter(|c| c.review.by == UNREVIEWED)
        .count();
    if unreviewed > 0 {
        summary.truncate(summary.trim_end().len());
        let _ = writeln!(
            summary,
            " {unreviewed} of them are not reviewed yet (`review.by: {UNREVIEWED}`)."
        );
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use teigeodata_core::PlaceKind;

    use super::{
        Clusters, Correction, Op, Outcome, apply, canonical, hash, link_clusters, overdue,
        previous_release_month, validate,
    };
    use crate::build::sources::Rec;
    use crate::build::{Place, PlaceName};

    const FILE: &str = r#"
schema: teigeodata/correction/v1
id: np-0001
target: geonames:1
op: add-name
name: { text: "𑐫𑐾𑑃", lang: new, type: local }
guard: { record: geonames:1, field: name, value_hash: HASH }
evidence:
  - { source: "Wikipedia, Kathmandu infobox", url: "https://en.wikipedia.org/wiki/Kathmandu", licence_class: share-alike }
created: 2026-11-09
author: maintainer
review: { by: maintainer, on: 2026-11-09 }
"#;

    fn correction(op_yaml: &str, guarded: &str, created: &str) -> Correction {
        let text = FILE
            .replace("HASH", &hash(guarded))
            .replace("2026-11-09\nauthor", &format!("{created}\nauthor"))
            .replace(
                "op: add-name\nname: { text: \"𑐫𑐾𑑃\", lang: new, type: local }",
                op_yaml,
            );
        let c: Correction = serde_saphyr::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(validate(&c, "np-0001"), Ok(()));
        c
    }

    fn add() -> &'static str {
        "op: add-name\nname: { text: \"𑐫𑐾𑑃\", lang: new, type: local }"
    }

    fn place(key: &str, names: &[&str], sources: &[&str]) -> Place {
        Place {
            key: key.into(),
            kind: PlaceKind::Settlement,
            admin_level: 0,
            flags: 0,
            point: (853_000_000, 277_000_000),
            names: names
                .iter()
                .map(|n| PlaceName {
                    text: (*n).into(),
                    lang: None,
                    kind: None,
                })
                .collect(),
            population: None,
            parent: None,
            sources: sources.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn records(name: &str) -> BTreeMap<String, Rec> {
        BTreeMap::from([(
            "geonames:1".to_string(),
            Rec {
                kind: "populated_place".into(),
                class: "P.PPLC".into(),
                name: name.into(),
                point: Some((853_000_000, 277_000_000)),
                population: Some(5),
            },
        )])
    }

    fn outcome(c: &Correction, places: &mut [Place], name: &str, lite: bool) -> Outcome {
        let out = apply(std::slice::from_ref(c), places, &records(name), &[], lite);
        out.unwrap_or_else(|e| panic!("{e}")).remove(0).1
    }

    #[test]
    fn a_holding_guard_applies_and_a_changed_one_is_stale() {
        let c = correction(add(), "Kathmandu", "2026-11-09");
        let mut places = vec![place("geonames:1", &["Kathmandu"], &["geonames:1"])];
        assert_eq!(
            outcome(&c, &mut places, "Kathmandu", false),
            Outcome::Applied
        );
        assert!(places[0].names.iter().any(|n| n.text == "𑐫𑐾𑑃"));
        // Applied again, the place has the name already: the sources agree, so it retires.
        assert_eq!(
            outcome(&c, &mut places, "Kathmandu", false),
            Outcome::Retired
        );
        let mut fresh = vec![place("geonames:1", &["Kathmandu"], &["geonames:1"])];
        let stale = outcome(&c, &mut fresh, "Kathmandu Metropolitan", false);
        assert!(matches!(stale, Outcome::Stale(ref w) if w.contains("Kathmandu Metropolitan")));
        assert_eq!(fresh[0].names.len(), 1, "a stale correction is not applied");
    }

    #[test]
    fn missing_targets_and_lite_evidence() {
        let c = correction(add(), "Kathmandu", "2026-11-09");
        let mut none = vec![place("geonames:2", &["Patan"], &["geonames:2"])];
        assert!(matches!(
            outcome(&c, &mut none, "Kathmandu", false),
            Outcome::Stale(_)
        ));
        let mut places = vec![place("geonames:1", &["Kathmandu"], &["geonames:1"])];
        assert_eq!(
            outcome(&c, &mut places, "Kathmandu", true),
            Outcome::NotLite
        );
        let gone = apply(
            std::slice::from_ref(&c),
            &mut places,
            &BTreeMap::new(),
            &[],
            false,
        );
        assert!(matches!(gone.as_deref(), Ok([(_, Outcome::Stale(_))])));
    }

    #[test]
    fn same_as_merges_and_the_canonical_name_stays() {
        let c = correction("op: same-as\nother: wikidata:Q9", "Kathmandu", "2026-11-09");
        let recs = [records("Kathmandu")];
        let mut clusters: Clusters = BTreeMap::from([
            (
                "geonames:1".to_string(),
                vec![(0, "geonames:1".to_string())],
            ),
            (
                "wikidata:Q9".to_string(),
                vec![(1, "wikidata:Q9".to_string())],
            ),
        ]);
        let first = link_clusters(std::slice::from_ref(&c), &mut clusters, &recs, false);
        assert_eq!(first, [("np-0001".to_string(), Outcome::Applied)]);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters["geonames:1"].len(), 2);
        let again = link_clusters(std::slice::from_ref(&c), &mut clusters, &recs, false);
        assert_eq!(again[0].1, Outcome::Retired);
        // A changed guard leaves the clusters as they are.
        let mut apart: Clusters = BTreeMap::from([
            (
                "geonames:1".to_string(),
                vec![(0, "geonames:1".to_string())],
            ),
            (
                "wikidata:Q9".to_string(),
                vec![(1, "wikidata:Q9".to_string())],
            ),
        ]);
        let changed = [records("Kathmandu Metropolitan")];
        let stale = link_clusters(std::slice::from_ref(&c), &mut apart, &changed, false);
        assert!(matches!(stale[0].1, Outcome::Stale(_)));
        assert_eq!(apart.len(), 2);
        // `apply` leaves same-as to the clusters.
        let mut places = vec![place("geonames:1", &["Kathmandu"], &["geonames:1"])];
        let none = apply(
            std::slice::from_ref(&c),
            &mut places,
            &records("Kathmandu"),
            &[],
            false,
        );
        assert_eq!(none, Ok(Vec::new()));

        let r = correction(
            "op: remove-name\ntext: Kathmandu",
            "Kathmandu",
            "2026-11-09",
        );
        let mut one = vec![place("geonames:1", &["Kathmandu", "Ktm"], &["geonames:1"])];
        assert!(apply(&[r], &mut one, &records("Kathmandu"), &[], false).is_err());
    }

    #[test]
    fn g_corr_fails_only_old_stale_corrections() {
        assert_eq!(
            previous_release_month("2026.11.0").as_deref(),
            Some("2026-10-01")
        );
        assert_eq!(
            previous_release_month("2027.01.2").as_deref(),
            Some("2026-12-01")
        );
        assert_eq!(previous_release_month("2026.13.0"), None);
        let old = correction(add(), "Kathmandu", "2026-09-30");
        let new = correction(add(), "Kathmandu", "2026-10-01");
        let stale = Outcome::Stale("x".into());
        let outcomes = [
            ("np-0001".to_string(), stale.clone()),
            ("np-0001".to_string(), stale),
        ];
        assert_eq!(
            overdue(&[old.clone(), new], &outcomes, "2026.11.0"),
            ["np-0001"]
        );
        let applied = [("np-0001".to_string(), Outcome::Applied)];
        assert_eq!(overdue(&[old], &applied, "2026.11.0"), Vec::<&str>::new());
    }

    #[test]
    fn files_are_checked() {
        let c = correction(add(), "Kathmandu", "2026-11-09");
        assert!(validate(&c, "np-0002").is_err(), "the id is the file name");
        let mut bad = c.clone();
        bad.guard.value_hash = "abc".into();
        assert!(validate(&bad, "np-0001").is_err());
        let mut bad = c.clone();
        bad.op = Op::SetPoint {
            lat: 91.0,
            lon: 0.0,
        };
        assert!(validate(&bad, "np-0001").is_err());
        let mut bad = c;
        bad.evidence[0].url = Some("http://example.org".into());
        assert!(validate(&bad, "np-0001").is_err());
        let r = &records("Kathmandu")["geonames:1"];
        assert_eq!(
            canonical(r, "point").as_deref(),
            Some("277000000,853000000")
        );
        assert_eq!(canonical(r, "population").as_deref(), Some("5"));
        assert_eq!(canonical(r, "elevation"), None);
        assert!(
            serde_saphyr::from_str::<Correction>(&FILE.replace("op: add-name", "op: rename"))
                .is_err()
        );
        // A field of another op, or an unknown field, is refused.
        let extra = FILE.replace("op: add-name", "op: add-name\nother: wikidata:Q1");
        assert!(serde_saphyr::from_str::<Correction>(&extra).is_err());
        let typo = FILE.replace("author:", "writer: x\nauthor:");
        assert!(serde_saphyr::from_str::<Correction>(&typo).is_err());
    }
}
