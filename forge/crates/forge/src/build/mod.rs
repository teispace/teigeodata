//! The `build` stage: one country's pack from the stages before it (phase 3 step 3.1, D-092).
//!
//! ```text
//! forge build <cc> --release <version> --admin <boundary-source> [--tz <timezone-source>]
//!       [--lite] [--key <seed-file>] [--ids <registry>] [--core <buildings-run>]
//!       <points-source> <points-source> […]
//! ```
//!
//! The points sources are the conflated run (`forge conflate <cc> <the same sources>`), in
//! priority order. A place takes the canonical name of its first source's record, and its
//! point from the `core` stage with `--core` (the consensus with the settlement core, D-091),
//! else its first source's point. Places are:
//!
//! - the country, from the profile's Wikidata item;
//! - every admin unit of the boundary source, at its own level, at a point inside its polygon,
//!   with its parent the unit of the level above that holds that point;
//! - one settlement or locality per cluster holding such records, with its parent the deepest
//!   admin unit that holds its point;
//! - every timezone zone whose box meets the country's (D-093), at a point inside it.
//!
//! Rows are in Hilbert order, ties broken by the place's key. IDs come from the ID registry
//! (`--ids`, the previous build's `<cc>-ids-<build>.tsv`): places keep their serials by shared
//! source keys and new ones are allocated in row order (D-075, D-094). The same inputs give the
//! same pack, byte for byte. `--lite` refuses every
//! source outside the attribution-only licence classes (`CLEAN_ROOM.md` rule 2) before reading
//! it, and leaves out the sections that would need one. Output:
//! `data/build/pack/<cc>/<cc>-places-<build>.tgd`, with `summary-<build>.md` and the updated
//! registry `<cc>-ids-<build>.tsv` beside it.

mod ids;
mod pack;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use arrow_array::{Array, Int64Array, StringArray};
use teigeodata_core::PlaceKind;
use teigeodata_format::names::{NameType, Tag};
use teigeodata_names::{PhoneticKey, fold, script_of};

use crate::fetch::store_path;
use crate::geom::{GridIndex, Polygon};
use crate::lock::Lock;
use crate::read::boundaries;
use crate::record::{column, for_each_batch};

/// The licence classes the lite build may read (`CLEAN_ROOM.md` rule 2).
pub(crate) const LITE: [&str; 7] = [
    "public-domain",
    "cc0",
    "cc-by",
    "cc-by-igo",
    "odc-by",
    "godl",
    "cdla-permissive",
];

/// Zones that win an overlap, before the source's order (D-084): the Nepal perspective.
const TZ_FIRST: [&str; 1] = ["Asia/Kathmandu"];

/// What the command line asks for.
#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) cc: String,
    pub(crate) release: String,
    pub(crate) admin: String,
    pub(crate) tz: Option<String>,
    pub(crate) lite: bool,
    pub(crate) key: Option<String>,
    /// The ID registry of the previous build or release; none for a country's first build.
    pub(crate) ids: Option<String>,
    /// The buildings run of the `core` stage whose consensus points the places take.
    pub(crate) core: Option<String>,
    pub(crate) sources: Vec<String>,
}

impl Args {
    pub(crate) fn parse(args: &[String]) -> Result<Self, String> {
        let usage = "usage: forge build <cc> --release <version> --admin <boundary-source> [--tz <timezone-source>] [--lite] [--key <seed-file>] [--ids <registry>] [--core <buildings-run>] <points-source> <points-source> [...]";
        let mut it = args.iter();
        let cc = it.next().ok_or(usage)?.clone();
        let (mut release, mut admin, mut tz, mut key, mut ids, mut core, mut lite) =
            (None, None, None, None, None, None, false);
        let mut sources = Vec::new();
        while let Some(a) = it.next() {
            let mut value = || it.next().cloned().ok_or(usage);
            match a.as_str() {
                "--release" => release = Some(value()?),
                "--admin" => admin = Some(value()?),
                "--tz" => tz = Some(value()?),
                "--key" => key = Some(value()?),
                "--ids" => ids = Some(value()?),
                "--core" => core = Some(value()?),
                "--lite" => lite = true,
                s if s.starts_with("--") => return Err(format!("unknown flag `{s}`; {usage}")),
                s => sources.push(s.to_string()),
            }
        }
        if sources.is_empty() {
            return Err(usage.into());
        }
        Ok(Self {
            cc,
            release: release.ok_or(usage)?,
            admin: admin.ok_or(usage)?,
            tz,
            lite,
            key,
            ids,
            core,
            sources,
        })
    }

    pub(crate) fn build_name(&self) -> &'static str {
        if self.lite { "lite" } else { "main" }
    }
}

/// One name of a place, before encoding.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PlaceName {
    pub(crate) text: String,
    pub(crate) lang: Option<String>,
    pub(crate) kind: Option<NameType>,
}

/// One place, before rows are assigned.
#[derive(Clone, Debug)]
pub(crate) struct Place {
    /// What it came from (`codab:NP301`, a cluster name, `tz:Asia/Kathmandu`): the tie-break
    /// of the row order, and stable across builds of the same inputs.
    pub(crate) key: String,
    pub(crate) kind: PlaceKind,
    pub(crate) admin_level: u8,
    /// (lon, lat) on the 1e-7° grid.
    pub(crate) point: (i32, i32),
    /// Canonical first.
    pub(crate) names: Vec<PlaceName>,
    pub(crate) population: Option<i64>,
    /// The key of the parent place.
    pub(crate) parent: Option<String>,
    /// The source keys it was made of (`osm:node/1`, `geonames:2`, `admin:NP0101`): what the ID
    /// registry matches across builds.
    pub(crate) sources: Vec<String>,
}

/// A record of a points source, as the build needs it.
#[derive(Clone, Debug)]
struct Rec {
    kind: String,
    name: String,
    point: Option<(i32, i32)>,
    population: Option<i64>,
}

/// `forge build …`.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let a = Args::parse(args)?;
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let licence = |id: &str| lock.get(id).map(|s| s.licence_class.clone());
    let mut skipped = Vec::new();
    let mut allowed = |id: &str| -> Result<bool, String> {
        let class = licence(id)?;
        let ok = !a.lite || LITE.contains(&class.as_str());
        if !ok {
            skipped.push(format!("{id} ({class})"));
        }
        Ok(ok)
    };
    // The schedule-time licence gate (G-lic): a source the build may not read is never opened.
    let sources: Vec<String> = a
        .sources
        .iter()
        .filter_map(|s| allowed(s).map(|ok| ok.then(|| s.clone())).transpose())
        .collect::<Result<_, _>>()?;
    if !allowed(&a.admin)? {
        return Err(format!(
            "the boundary source {} is not allowed in this build",
            a.admin
        ));
    }
    let tz = match &a.tz {
        Some(id) if allowed(id)? => Some(id.clone()),
        _ => None,
    };

    let build = root.join("data/build");
    let profile = Profile::read(root, &a.cc)?;
    let units = admin_places(root, &lock, &a.admin)?;
    let mut places: Vec<Place> = units.places;
    let core = match &a.core {
        Some(b) => core_points(&build, &a.sources, b)?,
        None => BTreeMap::new(),
    };
    let Clustered {
        places: clustered,
        unnamed: unnamed_clusters,
        held,
    } = cluster_places(&build, &a.sources, &sources, &units.levels, &core)?;
    places.extend(clustered);
    if let Some(country) = country_place(&build, &sources, &profile)? {
        places.push(country);
    }
    let coverage = units.coverage.ok_or("the boundary source has no units")?;
    let zones = match &tz {
        Some(id) => zone_places(root, &lock, id, coverage)?,
        None => Zones::default(),
    };
    places.extend(zones.places.iter().cloned());
    // A place is named: names that cannot be stored are dropped, then places left without a
    // canonical name (a source record with an empty name).
    for p in &mut places {
        p.names.retain(|n| pack::fits(&n.text));
    }
    let before = places.len();
    places.retain(|p| !p.names.is_empty());
    let unnamed = before - places.len() + unnamed_clusters;

    let out_dir = build.join("pack").join(&a.cc);
    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let file = out_dir.join(format!("{}-places-{}.tgd", a.cc, a.build_name()));
    let registry = registry(root, a.ids.as_deref())?;
    let report = pack::write(&pack::Input {
        root,
        args: &a,
        places: &places,
        profile: &profile,
        zones: &zones,
        coverage,
        file: &file,
        registry: &registry,
        ids_file: &out_dir.join(format!("{}-ids-{}.tsv", a.cc, a.build_name())),
    })?;
    let mut summary = format!(
        "## build: {} {}\n\n{} places from {}; boundaries {}; timezones {}.\n",
        a.cc,
        a.build_name(),
        places.len(),
        sources.join(", "),
        a.admin,
        tz.as_deref().unwrap_or("none"),
    );
    notes(&mut summary, held, unnamed, &skipped);
    summary.push('\n');
    summary.push_str(&report);
    fs::write(
        out_dir.join(format!("summary-{}.md", a.build_name())),
        &summary,
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "{summary}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(&file).display(),
        t.elapsed()
    ))
}

/// The summary's notes: places held at their seed point, left out, and sources not read.
fn notes(summary: &mut String, held: usize, unnamed: usize, skipped: &[String]) {
    if held > 0 {
        let _ = writeln!(
            *summary,
            "Kept the seed point: {held} places whose core point lies outside every admin unit."
        );
    }
    if unnamed > 0 {
        let _ = writeln!(*summary, "Left out: {unnamed} places with no usable name.");
    }
    if !skipped.is_empty() {
        let _ = writeln!(*summary, "Not read (licence): {}.", skipped.join(", "));
    }
}

/// The ID registry at `path` (relative to the root), or an empty one for a first build.
fn registry(root: &Path, path: Option<&str>) -> Result<ids::Registry, String> {
    let Some(path) = path else {
        return Ok(ids::Registry::default());
    };
    let path = root.join(path);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    ids::Registry::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The consensus points of a `core` run, by cluster.
fn core_points(
    build: &Path,
    run: &[String],
    buildings: &str,
) -> Result<BTreeMap<String, (i32, i32)>, String> {
    let path = build
        .join("core")
        .join(run.join("+"))
        .join(buildings)
        .join("points.parquet");
    let mut out = BTreeMap::new();
    for_each_batch(&path, |b| {
        let cl = column::<StringArray>(b, "cluster")?;
        let lon = column::<arrow_array::Int32Array>(b, "lon_e7")?;
        let lat = column::<arrow_array::Int32Array>(b, "lat_e7")?;
        for i in 0..b.num_rows() {
            out.insert(cl.value(i).to_string(), (lon.value(i), lat.value(i)));
        }
        Ok(())
    })
    .map_err(|e| format!("{}: {e}; run forge core first", path.display()))?;
    Ok(out)
}

/// What the build reads from the country profile.
#[derive(Debug, Default)]
pub(crate) struct Profile {
    pub(crate) phonetic: Option<PhoneticKey>,
    /// Folded.
    pub(crate) droppable: Vec<String>,
    pub(crate) wikidata: Option<String>,
}

#[derive(serde::Deserialize)]
struct ProfileFile {
    #[serde(default)]
    phonetic_key: Option<String>,
    #[serde(default)]
    droppable_suffixes: Vec<Term>,
    #[serde(default)]
    wikidata: Option<String>,
}

#[derive(serde::Deserialize)]
struct Term {
    text: String,
}

impl Profile {
    fn read(root: &Path, cc: &str) -> Result<Self, String> {
        let path = root.join("data/countries").join(cc).join("profile.yaml");
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let p: ProfileFile =
            serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let phonetic = match p.phonetic_key.as_deref() {
            Some(name) => Some(
                PhoneticKey::from_name(name)
                    .ok_or_else(|| format!("{}: unknown phonetic key {name}", path.display()))?,
            ),
            None => None,
        };
        Ok(Self {
            phonetic,
            droppable: p.droppable_suffixes.iter().map(|t| fold(&t.text)).collect(),
            wikidata: p.wikidata,
        })
    }
}

/// The admin places, their polygons level by level, and the box of the whole set.
struct Units {
    places: Vec<Place>,
    /// Coarsest level first.
    levels: Vec<Level>,
    coverage: Option<(i32, i32, i32, i32)>,
}

/// One admin level's polygons with a grid index over their boxes.
struct Level {
    keys: Vec<String>,
    polys: Vec<Polygon>,
    index: GridIndex,
}

impl Level {
    /// The key of the unit holding `p`.
    fn find(&self, p: (i32, i32)) -> Option<&String> {
        self.index
            .candidates(p.0, p.1)
            .find(|&i| {
                self.polys
                    .get(i)
                    .is_some_and(|poly| poly.contains(p.0, p.1))
            })
            .and_then(|i| self.keys.get(i))
    }
}

/// 0.1° cells: a few polygons per cell at local-unit scale (as `pip`).
const CELL_E7: i32 = 1_000_000;

fn admin_places(root: &Path, lock: &Lock, source: &str) -> Result<Units, String> {
    let zip = store_path(root, lock.get(source)?);
    let mut numbers = boundaries::levels(&zip)?;
    numbers.sort_unstable();
    let mut places = Vec::new();
    let mut levels: Vec<Level> = Vec::new();
    let mut coverage: Option<(i32, i32, i32, i32)> = None;
    for &level in numbers.iter().filter(|&&l| l > 0) {
        let mut units = boundaries::read(&zip, level)?;
        units.sort_by(|a, b| a.pcode.cmp(&b.pcode));
        let (mut keys, mut polys) = (Vec::new(), Vec::new());
        for u in units {
            // National codes, not the source ID: a new boundary release keeps the IDs.
            let key = format!("admin:{}", u.pcode);
            let point = u
                .polygon
                .interior_point()
                .ok_or_else(|| format!("{key}: no point inside its polygon"))?;
            if let Some(b) = u.polygon.bbox() {
                coverage = Some(coverage.map_or(b, |c| {
                    (c.0.min(b.0), c.1.min(b.1), c.2.max(b.2), c.3.max(b.3))
                }));
            }
            // The parent is the unit one level up that holds this unit's point.
            let parent = levels.last().and_then(|l| l.find(point)).cloned();
            places.push(Place {
                key: key.clone(),
                kind: PlaceKind::Admin,
                admin_level: level,
                point,
                names: vec![PlaceName {
                    text: u.name.clone(),
                    lang: None,
                    kind: Some(NameType::Official),
                }],
                population: None,
                parent,
                sources: vec![key.clone()],
            });
            keys.push(key);
            polys.push(u.polygon);
        }
        let index = GridIndex::new(&polys, CELL_E7);
        levels.push(Level { keys, polys, index });
    }
    Ok(Units {
        places,
        levels,
        coverage,
    })
}

/// The deepest admin unit holding `p`, by key.
fn deepest(levels: &[Level], p: (i32, i32)) -> Option<String> {
    levels.iter().rev().find_map(|l| l.find(p)).cloned()
}

fn read_records(build: &Path, source: &str) -> Result<BTreeMap<String, Rec>, String> {
    let mut out = BTreeMap::new();
    for_each_batch(
        &build.join("read").join(source).join("records.parquet"),
        |b| {
            let ids = column::<StringArray>(b, "id")?;
            let kinds = column::<StringArray>(b, "kind")?;
            let names = column::<StringArray>(b, "name")?;
            let lat = column::<arrow_array::Int32Array>(b, "lat_e7")?;
            let lon = column::<arrow_array::Int32Array>(b, "lon_e7")?;
            let pop = column::<Int64Array>(b, "population")?;
            for i in 0..b.num_rows() {
                let point =
                    (lat.is_valid(i) && lon.is_valid(i)).then(|| (lon.value(i), lat.value(i)));
                out.insert(
                    ids.value(i).to_string(),
                    Rec {
                        kind: kinds.value(i).to_string(),
                        name: names.value(i).to_string(),
                        point,
                        population: pop.is_valid(i).then(|| pop.value(i)),
                    },
                );
            }
            Ok(())
        },
    )?;
    Ok(out)
}

fn read_names(build: &Path, source: &str) -> Result<BTreeMap<String, Vec<PlaceName>>, String> {
    let mut out: BTreeMap<String, Vec<PlaceName>> = BTreeMap::new();
    for_each_batch(
        &build.join("read").join(source).join("names.parquet"),
        |b| {
            let ids = column::<StringArray>(b, "id")?;
            let text = column::<StringArray>(b, "text")?;
            let lang = column::<StringArray>(b, "lang")?;
            let kind = column::<StringArray>(b, "kind")?;
            for i in 0..b.num_rows() {
                out.entry(ids.value(i).to_string())
                    .or_default()
                    .push(PlaceName {
                        text: text.value(i).to_string(),
                        lang: Some(lang.value(i))
                            .filter(|l| !l.is_empty())
                            .map(str::to_string),
                        kind: name_type(kind.value(i)),
                    });
            }
            Ok(())
        },
    )?;
    Ok(out)
}

/// A source's name kind as a NAMES type (`None` for the plain primary name).
fn name_type(kind: &str) -> Option<NameType> {
    match kind {
        "official" => Some(NameType::Official),
        "alias" | "alternate" | "short" => Some(NameType::Alt),
        "colloquial" => Some(NameType::Colloquial),
        "old" => Some(NameType::Historical),
        _ => None,
    }
}

/// One place per cluster holding a settlement or locality record. `run` is the conflated run
/// (its order names the run's folder); `read` the sources this build may read, in priority
/// order.
/// The settlements and localities of the conflated clusters.
struct Clustered {
    places: Vec<Place>,
    /// Clusters left out for want of a name that can be stored.
    unnamed: usize,
    /// Places that kept their seed point because the core point lies outside every admin unit.
    held: usize,
}

fn cluster_places(
    build: &Path,
    run: &[String],
    read: &[String],
    levels: &[Level],
    core: &BTreeMap<String, (i32, i32)>,
) -> Result<Clustered, String> {
    let mut records: Vec<BTreeMap<String, Rec>> = Vec::new();
    let mut names: Vec<BTreeMap<String, Vec<PlaceName>>> = Vec::new();
    for s in read {
        records.push(read_records(build, s)?);
        names.push(read_names(build, s)?);
    }
    // Members by cluster; a source the build may not read contributes nothing.
    let mut clusters: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    let path = build
        .join("conflate")
        .join(run.join("+"))
        .join("clusters.parquet");
    for_each_batch(&path, |b| {
        let ids = column::<StringArray>(b, "id")?;
        let cl = column::<StringArray>(b, "cluster")?;
        for i in 0..b.num_rows() {
            let id = ids.value(i);
            if let Some(src) = records.iter().position(|r| r.contains_key(id)) {
                clusters
                    .entry(cl.value(i).to_string())
                    .or_default()
                    .push((src, id.to_string()));
            }
        }
        Ok(())
    })?;
    let (mut out, mut unnamed, mut held) = (Vec::new(), 0_usize, 0_usize);
    for (cluster, mut members) in clusters {
        members.sort();
        let recs: Vec<(&Rec, &str, usize)> = members
            .iter()
            .filter_map(|(s, id)| Some((records.get(*s)?.get(id)?, id.as_str(), *s)))
            .collect();
        let kind = if recs.iter().any(|(r, _, _)| r.kind == "populated_place") {
            PlaceKind::Settlement
        } else if recs.iter().any(|(r, _, _)| r.kind == "locality") {
            PlaceKind::Locality
        } else {
            continue;
        };
        // The first source's record with a point gives the point and the canonical name.
        let Some(&(first, _, _)) = recs.iter().find(|(r, _, _)| r.point.is_some()) else {
            continue;
        };
        let Some(point) = first.point else { continue };
        // The consensus point of the `core` stage, when the build reads one. A core that would
        // take the place out of every admin unit its seed point is in (across the border, as
        // buildings on the far side pull it) is refused: the place keeps its seed point.
        let point = match core.get(&cluster) {
            Some(&c) if deepest(levels, c).is_some() || deepest(levels, point).is_none() => c,
            Some(_) => {
                held += 1;
                point
            }
            None => point,
        };
        let mut rest: BTreeSet<PlaceName> = BTreeSet::new();
        for (_, id, s) in &recs {
            for n in names.get(*s).and_then(|m| m.get(*id)).into_iter().flatten() {
                rest.insert(n.clone());
            }
        }
        // The canonical name: the first record's name, in source priority, that can be
        // stored; else the first of the other names that can.
        let Some(canonical) = recs
            .iter()
            .map(|(r, _, _)| r.name.as_str())
            .chain(rest.iter().map(|n| n.text.as_str()))
            .find(|n| pack::fits(n))
            .map(str::to_string)
        else {
            unnamed += 1;
            continue;
        };
        let mut all: Vec<PlaceName> = vec![PlaceName {
            text: canonical.clone(),
            lang: None,
            kind: None,
        }];
        let mut seen: BTreeSet<String> = BTreeSet::from([canonical]);
        for n in rest {
            if seen.insert(n.text.clone()) {
                all.push(n);
            }
        }
        out.push(Place {
            key: cluster,
            kind,
            admin_level: 0,
            point,
            names: all,
            population: recs.iter().find_map(|(r, _, _)| r.population),
            parent: deepest(levels, point),
            sources: members.into_iter().map(|(_, id)| id).collect(),
        });
    }
    Ok(Clustered {
        places: out,
        unnamed,
        held,
    })
}

/// The country, from the record of its Wikidata item, when a source has it.
fn country_place(
    build: &Path,
    read: &[String],
    profile: &Profile,
) -> Result<Option<Place>, String> {
    let Some(q) = &profile.wikidata else {
        return Ok(None);
    };
    let id = format!("wikidata:{q}");
    for s in read {
        let recs = read_records(build, s)?;
        if let Some(r) = recs.get(&id)
            && let Some(point) = r.point
        {
            let mut names = vec![PlaceName {
                text: r.name.clone(),
                lang: None,
                kind: Some(NameType::Official),
            }];
            let mut seen = BTreeSet::from([r.name.clone()]);
            for n in read_names(build, s)?.remove(&id).unwrap_or_default() {
                if seen.insert(n.text.clone()) {
                    names.push(n);
                }
            }
            return Ok(Some(Place {
                sources: vec![id.clone()],
                key: id,
                kind: PlaceKind::Country,
                admin_level: 0,
                point,
                names,
                population: r.population,
                parent: None,
            }));
        }
    }
    Ok(None)
}

/// The timezone zones of a pack: their places and rings, in precedence order.
#[derive(Debug, Default)]
pub(crate) struct Zones {
    pub(crate) places: Vec<Place>,
    pub(crate) rings: Vec<Vec<Vec<[i32; 2]>>>,
}

#[derive(serde::Deserialize)]
struct ZoneCollection {
    features: Vec<ZoneFeature>,
}

#[derive(serde::Deserialize)]
struct ZoneFeature {
    properties: ZoneProps,
    geometry: ZoneGeometry,
}

#[derive(serde::Deserialize)]
struct ZoneProps {
    tzid: String,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum ZoneGeometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

/// The zones of timezone-boundary-builder whose box meets `coverage`, `TZ_FIRST` first, then
/// in the source's order.
fn zone_places(
    root: &Path,
    lock: &Lock,
    source: &str,
    coverage: (i32, i32, i32, i32),
) -> Result<Zones, String> {
    use std::io::Read as _;
    let zip_path = store_path(root, lock.get(source)?);
    let file = fs::File::open(&zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let name = zip
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.ends_with(".json") || n.ends_with(".geojson"))
        .map(|n| n.to_string())
        .ok_or("no GeoJSON in the timezone archive")?;
    let mut bytes = Vec::new();
    zip.by_name(&name)
        .map_err(|e| e.to_string())?
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let fc: ZoneCollection = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    drop(bytes);
    let mut zones: Vec<(String, Vec<Vec<[i32; 2]>>)> = Vec::new();
    for f in fc.features {
        let polys = match f.geometry {
            ZoneGeometry::Polygon(p) => vec![p],
            ZoneGeometry::MultiPolygon(m) => m,
        };
        let rings: Vec<Vec<[i32; 2]>> = polys
            .into_iter()
            .flatten()
            .map(|r| {
                r.iter()
                    .filter_map(|c| Some([crate::record::e7(c[0])?, crate::record::e7(c[1])?]))
                    .collect()
            })
            .collect();
        let meets = rings
            .iter()
            .flatten()
            .fold(None, |b: Option<(i32, i32, i32, i32)>, p| {
                Some(b.map_or((p[0], p[1], p[0], p[1]), |b| {
                    (b.0.min(p[0]), b.1.min(p[1]), b.2.max(p[0]), b.3.max(p[1]))
                }))
            });
        if let Some(b) = meets
            && b.0 <= coverage.2
            && coverage.0 <= b.2
            && b.1 <= coverage.3
            && coverage.1 <= b.3
        {
            zones.push((f.properties.tzid, rings));
        }
    }
    zones.sort_by_key(|(id, _)| !TZ_FIRST.contains(&id.as_str()));
    let mut out = Zones::default();
    for (tzid, rings) in zones {
        let poly = Polygon {
            rings: rings
                .iter()
                .map(|r| r.iter().map(|p| (p[0], p[1])).collect())
                .collect(),
        };
        // A point inside the zone (not necessarily inside the coverage).
        let point = poly
            .interior_point()
            .ok_or_else(|| format!("{tzid}: no point inside the zone"))?;
        out.places.push(Place {
            key: format!("tz:{tzid}"),
            sources: vec![format!("tz:{tzid}")],
            kind: PlaceKind::TimezoneArea,
            admin_level: 0,
            point,
            names: vec![PlaceName {
                text: tzid,
                lang: None,
                kind: Some(NameType::Official),
            }],
            population: None,
            parent: None,
        });
        out.rings.push(rings);
    }
    Ok(out)
}

/// The NAMES tags of a name: its language (the primary subtag) and its script.
pub(crate) fn tags(n: &PlaceName) -> (Option<Tag>, Option<Tag>) {
    let lang = n
        .lang
        .as_deref()
        .and_then(|l| l.split(['-', '_']).next())
        .and_then(Tag::new);
    let script = Tag::new(script_of(&n.text).short_name());
    (lang, script)
}
