//! The `build` stage: one country's pack from the stages before it (phase 3 step 3.1, D-092).
//!
//! ```text
//! forge build <cc> --release <version> --admin <boundary-source> [--tz <timezone-source>]
//!       [--lite] [--key <seed-file>] [--ids <registry>] [--core <buildings-run>]
//!       [--dem <tileset>] <points-source> <points-source> […]
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
//! With `--dem`, every place but a timezone area takes its elevation from the tileset's
//! `dem-section` at its final point (D-111, [`elevation`]).
//!
//! The postal codes of `data/countries/<cc>/postal.yaml`, checked against their source table,
//! link to the admin places (D-105).
//!
//! Rows are in Hilbert order, ties broken by the place's key. IDs come from the ID registry
//! (`--ids`, the previous build's `<cc>-ids-<build>.tsv`): places keep their serials by shared
//! source keys and new ones are allocated in row order (D-075, D-094). The same inputs give the
//! same pack, byte for byte. `--lite` refuses every
//! source outside the attribution-only licence classes (`CLEAN_ROOM.md` rule 2) before reading
//! it, and leaves out the sections that would need one. Output:
//! `data/build/pack/<cc>/<cc>-places-<build>.tgd`, with `summary-<build>.md` and the updated
//! registry `<cc>-ids-<build>.tsv` beside it.

mod admins;
pub(crate) mod baseline;
mod cluster;
mod corrections;
mod dedupe;
mod elevation;
pub(crate) mod embed;
pub(crate) mod geometry;
mod ids;
mod iso;
mod naming;
mod pack;
mod population;
mod postal;
mod sources;
mod split;
pub(crate) mod tz_rules;
mod units;
pub(crate) mod world;
mod world_overture;
mod world_tz;
mod zones;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use teigeodata_core::PlaceKind;
use teigeodata_format::names::{NameType, Tag};
use teigeodata_names::{PhoneticKey, fold, script_of};

use crate::lock::Lock;
use crate::names::Namer;

use cluster::{Clustered, cluster_places, link_admins};
use naming::Naming;
use sources::core_points;
use units::{Level, admin_places, country_place};
use zones::{Zones, zone_places};

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

/// What the command line asks for.
#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) cc: String,
    pub(crate) release: String,
    pub(crate) admin: String,
    pub(crate) tz: Option<String>,
    /// The zones that win an overlap (`--tz-first`, comma-separated; D-084, D-138).
    pub(crate) tz_first: Vec<String>,
    pub(crate) lite: bool,
    pub(crate) key: Option<String>,
    /// The ID registry of the previous build or release; none for a country's first build.
    pub(crate) ids: Option<String>,
    /// The buildings run of the `core` stage whose consensus points the places take.
    pub(crate) core: Option<String>,
    /// Whether the pack carries the admin geometry of the `topology` stage (ARCS, POLYGONS).
    pub(crate) topology: bool,
    /// The DEM tileset whose `dem-section` gives place elevations (D-111).
    pub(crate) dem: Option<String>,
    /// The population grid of admin units' modelled population (D-123).
    pub(crate) population: Option<String>,
    pub(crate) sources: Vec<String>,
}

impl Args {
    pub(crate) fn parse(args: &[String]) -> Result<Self, String> {
        let usage = "usage: forge build <cc> --release <version> --admin <boundary-source> [--tz <timezone-source>] [--tz-first <zone,...>] [--lite] [--key <seed-file>] [--ids <registry>] [--core <buildings-run>] [--topology] [--dem <tileset>] [--population <grid>] [--split] <points-source> <points-source> [...]";
        let mut it = args.iter();
        let cc = it.next().ok_or(usage)?.clone();
        let (mut release, mut admin, mut tz, mut key, mut ids, mut core, mut lite) =
            (None, None, None, None, None, None, false);
        let (mut topology, mut dem, mut population) = (false, None, None);
        let mut tz_first = Vec::new();
        let mut sources = Vec::new();
        while let Some(a) = it.next() {
            let mut value = || it.next().cloned().ok_or(usage);
            match a.as_str() {
                "--release" => release = Some(value()?),
                "--admin" => admin = Some(value()?),
                "--tz" => tz = Some(value()?),
                "--tz-first" => tz_first = value()?.split(',').map(str::to_string).collect(),
                "--key" => key = Some(value()?),
                "--ids" => ids = Some(value()?),
                "--core" => core = Some(value()?),
                "--dem" => dem = Some(value()?),
                "--population" => population = Some(value()?),
                "--lite" => lite = true,
                "--topology" => topology = true,
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
            tz_first,
            lite,
            key,
            ids,
            core,
            topology,
            dem,
            population,
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
    /// PLACES flag bits (`places::FLAG_FORMER`).
    pub(crate) flags: u32,
    /// (lon, lat) on the 1e-7° grid.
    pub(crate) point: (i32, i32),
    /// Canonical first.
    pub(crate) names: Vec<PlaceName>,
    pub(crate) population: Option<i64>,
    /// The best settlement rank of its records (`sources::settlement_rank`), 0 for none.
    pub(crate) rank: u8,
    /// The key of the parent place.
    pub(crate) parent: Option<String>,
    /// The source keys it was made of (`osm:node/1`, `geonames:2`, `admin:NP0101`): what the ID
    /// registry matches across builds.
    pub(crate) sources: Vec<String>,
}

/// The arguments and, with `--split` (D-191), the country's split. `--split` is not part of
/// `Args`: the build ID hashes `Args`, and the country pack must be the same bytes whether or
/// not its parts are written.
fn args_and_split(root: &Path, args: &[String]) -> Result<(Args, Option<split::Split>), String> {
    let with = args.iter().any(|a| a == "--split");
    let args: Vec<String> = args.iter().filter(|a| *a != "--split").cloned().collect();
    let a = Args::parse(&args)?;
    let split = with.then(|| split::read(root, &a.cc)).transpose()?;
    Ok((a, split))
}

/// Adds the country's own place (when its source has one) after the first `units` rows of
/// `places`, and parents the top units to it.
fn add_country(
    build: &Path,
    sources: &[String],
    profile: &Profile,
    (places, units): (&mut Vec<Place>, usize),
    naming: &mut Naming,
) -> Result<(), String> {
    let current = places.get(..units).unwrap_or_default();
    if let Some(country) = country_place(build, sources, profile, current, naming)? {
        let key = country.key.clone();
        places.push(country);
        units::parent_to_country(places, &key);
    }
    Ok(())
}

/// The admin source's shared-boundary topology when the build asks for it (`--topology`).
fn topology(root: &Path, a: &Args) -> Result<Option<geometry::Geometry>, String> {
    a.topology
        .then(|| geometry::load(root, &a.admin))
        .transpose()
}

/// What the pack stage reads beside the places: where it writes, the ID registry, the
/// topology, the timezones the sources state and the ISO 3166 codes.
struct PackInputs {
    file: PathBuf,
    ids_file: PathBuf,
    registry: ids::Registry,
    geometry: Option<geometry::Geometry>,
    stated_tz: BTreeMap<String, String>,
    /// The country's ISO 3166 codes (D-197), when it has `iso3166.yaml`.
    iso: Option<iso::Iso>,
}

impl PackInputs {
    fn read(root: &Path, a: &Args, sources: &[String], out_dir: &Path) -> Result<Self, String> {
        Ok(Self {
            file: out_dir.join(format!("{}-places-{}.tgd", a.cc, a.build_name())),
            ids_file: out_dir.join(format!("{}-ids-{}.tsv", a.cc, a.build_name())),
            registry: registry(root, a.ids.as_deref())?,
            geometry: topology(root, a)?,
            stated_tz: zones::stated(&root.join("data/build"), sources)?,
            iso: iso::read(root, &a.cc)?,
        })
    }
}

/// The country's area, its DEM's mask (D-192): the polygons of its first admin level.
fn first_level(levels: &[Level]) -> Vec<&crate::geom::Polygon> {
    levels
        .first()
        .map(|l| l.polys.iter().collect())
        .unwrap_or_default()
}

/// Writes the country pack and, with a split (D-191), its parts beside it, after removing any
/// parts an earlier build left there. Returns the report.
fn packs(
    input: &pack::Input<'_>,
    split: Option<(&split::Split, &[Level])>,
    out_dir: &Path,
) -> Result<String, String> {
    split::remove_parts(out_dir, &input.args.cc, input.args.build_name())?;
    let (mut report, country) = pack::write(input)?;
    if let Some((s, levels)) = split {
        report.push_str(&split::run(input, &country, s, levels, out_dir)?);
    }
    Ok(report)
}

/// `forge build …`.
#[allow(clippy::too_many_lines, reason = "the build's stages, in order")]
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let (a, split) = args_and_split(root, args)?;
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let g = gate(root, &a, &lock)?;

    let build = root.join("data/build");
    let profile = Profile::read(root, &a.cc)?;
    let units = admin_places(root, &lock, &a.admin)?;
    let core = a.core.as_ref().map_or_else(
        || Ok(BTreeMap::new()),
        |b| core_points(&build, &a.sources, b),
    )?;
    let namer = Namer::load(root, &a.cc, &profile.romanise)?;
    let fixes = corrections::load(root, &a.cc)?;
    let Clustered {
        linked,
        places: mut clustered,
        unnamed: unnamed_clusters,
        held,
        abroad,
        mut naming,
        mut admin,
    } = cluster_places(
        &build,
        &a.sources,
        &g.sources,
        &units,
        &core,
        (&namer, profile.phonetic),
        (&fixes, a.lite),
    )?;
    let merged = dedupe::merge_near(&mut clustered);
    let unit_count = units.places.len();
    let mut places: Vec<Place> = units.places;
    places.extend(clustered);
    add_country(
        &build,
        &g.sources,
        &profile,
        (&mut places, unit_count),
        &mut naming,
    )?;
    admin.added = link_admins(&mut places, std::mem::take(&mut admin.links));
    naming::drop_qualified(&mut places, &mut naming);
    let coverage = units.coverage.ok_or("the boundary source has no units")?;
    let modelled = population::run(g.grid.as_slice(), &units.levels, coverage, &mut places)?;
    let out_dir = build.join("pack").join(&a.cc);
    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let who = (
        fixes.as_slice(),
        linked.as_slice(),
        a.release.as_str(),
        a.lite,
    );
    let corrected = corrections::run(root, who, &g.sources, &mut places, &units.levels, &out_dir)?;
    let zones = zone_places(root, &lock, g.tz.as_deref(), coverage, &a)?;
    let rules = tz_rules::compile(root, &lock, g.tz.as_deref(), &zones)?;
    places.extend(zones.places.iter().cloned());
    let unnamed = keep_named(&mut places) + unnamed_clusters;
    let elevations = elevation::load(root, (&build, &g.sources), g.dem.as_deref(), &places)?;

    let f = PackInputs::read(root, &a, &g.sources, &out_dir)?;
    let input = pack::Input {
        root,
        args: &a,
        address: &a.cc,
        given: None,
        places: &places,
        profile: &profile,
        zones: &zones,
        coverage,
        file: &f.file,
        registry: &f.registry,
        ids_file: Some(&f.ids_file),
        geometry: f.geometry.as_ref(),
        postal: g.postal.as_ref(),
        elevation: elevations.as_ref(),
        // The country's area, its DEM's mask (D-192).
        outline: &first_level(&units.levels),
        attribution: &g.attribution,
        stated_tz: &f.stated_tz,
        iso: f.iso.as_ref(),
        tz_rules: rules.as_ref(),
    };
    let report = packs(
        &input,
        split.as_ref().map(|s| (s, units.levels.as_slice())),
        &out_dir,
    )?;
    let mut summary = header(&a, places.len(), &g.sources, g.tz.as_deref());
    notes(&mut summary, (held, abroad), unnamed, &g);
    summary.push_str(&naming.line());
    summary.push_str(&modelled.report);
    summary.push_str(&admin.line());
    summary.push_str(&dedupe::report(&merged, &out_dir, a.build_name())?);
    summary.push_str(&corrected);
    if let Some(e) = &elevations {
        summary.push_str(&e.summary(&build, &g.sources, &places)?);
    }
    naming.write(&out_dir, a.build_name())?;
    modelled.write(&out_dir, a.build_name())?;
    finish(
        (summary, &report, &g.attribution),
        (root, &out_dir, &f.file),
        a.build_name(),
        t,
    )
}

/// Writes `summary-<build>.md` (the build's lines, then the pack's report) and
/// `attribution-<build>.md` (the pack's notices, D-126), and returns the summary with the
/// pack's path and the time taken.
fn finish(
    (mut summary, report, attribution): (String, &str, &str),
    (root, out_dir, file): (&Path, &Path, &Path),
    build: &str,
    t: Instant,
) -> Result<String, String> {
    summary.push('\n');
    summary.push_str(report);
    fs::write(out_dir.join(format!("summary-{build}.md")), &summary).map_err(|e| e.to_string())?;
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    fs::write(
        out_dir.join(format!("attribution-{build}.md")),
        format!("# Attribution: {name}\n\n{attribution}\n"),
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "{summary}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(file).display(),
        t.elapsed()
    ))
}

/// `forge correction-guard`: the canonical value of a read record's field and the guard a
/// correction file states for it (ADR-0036).
pub(crate) fn correction_guard(
    root: &Path,
    source: &str,
    record: &str,
    field: &str,
) -> Result<String, String> {
    let records = sources::read_records(&root.join("data/build"), source)?;
    let rec = records
        .get(record)
        .ok_or_else(|| format!("{record} is not a record of {source}"))?;
    let value = corrections::canonical(rec, field)
        .ok_or_else(|| format!("{field} is not name, point or population"))?;
    Ok(format!(
        "guard: {{ record: {record}, field: {field}, value_hash: {} }}  # {value:?}",
        corrections::hash(&value)
    ))
}

/// The summary's first lines: the build, its places and its inputs.
fn header(a: &Args, places: usize, sources: &[String], tz: Option<&str>) -> String {
    format!(
        "## build: {} {}\n\n{places} places from {}; boundaries {}; timezones {}.\n",
        a.cc,
        a.build_name(),
        sources.join(", "),
        a.admin,
        tz.unwrap_or("none"),
    )
}

/// What the licence gate lets a build read.
struct Gated {
    sources: Vec<String>,
    tz: Option<String>,
    postal: Option<postal::Postal>,
    dem: Option<String>,
    /// The population grid's file, when the build reads one.
    grid: Option<PathBuf>,
    /// The sources not read, with their licence class.
    skipped: Vec<String>,
    /// The notices of every source the build reads, one per line, in reading order (D-126).
    attribution: String,
}

/// The schedule-time licence gate (G-lic): a source the build may not read is never opened.
fn gate(root: &Path, a: &Args, lock: &Lock) -> Result<Gated, String> {
    let mut skipped = Vec::new();
    let mut read: Vec<&str> = Vec::new();
    let mut allowed = |id: &str| -> Result<bool, String> {
        let source = lock.get(id)?;
        let class = &source.licence_class;
        let ok = !a.lite || LITE.contains(&class.as_str());
        if ok {
            read.push(source.id.as_str());
        } else {
            skipped.push(format!("{id} ({class})"));
        }
        Ok(ok)
    };
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
    // The zone rules come from the outlines' tzdb release (D-196): its data and code are read.
    if let Some(t) = &tz {
        for id in tz_rules::sources(lock, t)?.1 {
            if !allowed(&id)? {
                return Err(format!(
                    "the zone rules' source {id} is not allowed in this build"
                ));
            }
        }
    }
    let postal = postal::load(root, &a.cc, lock, &a.admin, &mut allowed)?;
    let dem = match &a.dem {
        Some(id) if allowed(id)? => Some(id.clone()),
        _ => None,
    };
    let grid = match &a.population {
        Some(id) if allowed(id)? => Some(lock.path(root, id)?),
        _ => None,
    };
    // The core run's buildings shape every place's point: each must be allowed.
    for id in a.core.iter().flat_map(|run| run.split('+')) {
        if !allowed(id)? {
            return Err(format!(
                "the core run {} reads {id}, which this build may not read",
                a.core.as_deref().unwrap_or_default()
            ));
        }
    }
    let attribution = notices(lock, &read)?;
    Ok(Gated {
        sources,
        tz,
        postal,
        dem,
        grid,
        skipped,
        attribution,
    })
}

/// The notices of the sources `read`, once each in order, one per line: the pack's
/// `attribution` (D-126). Refuses a set over the manifest's text limit.
fn notices(lock: &Lock, read: &[&str]) -> Result<String, String> {
    let mut lines: Vec<&str> = Vec::new();
    for id in read {
        let n = lock.get(id)?.attribution.trim();
        if n.is_empty() || n.contains('\n') {
            return Err(format!("{id}: its attribution must be one non-empty line"));
        }
        if !lines.contains(&n) {
            lines.push(n);
        }
    }
    let text = lines.join("\n");
    if text.len() > teigeodata_format::MAX_TEXT {
        return Err(format!(
            "the sources' notices are {} bytes, over the manifest's {}",
            text.len(),
            teigeodata_format::MAX_TEXT
        ));
    }
    Ok(text)
}

/// The summary's notes: postal codes left out, places held at their seed point, left out,
/// and sources not read.
fn notes(summary: &mut String, (held, abroad): (usize, usize), unnamed: usize, g: &Gated) {
    let skipped = &g.skipped;
    for n in g.postal.iter().flat_map(|p| &p.notes) {
        let _ = writeln!(*summary, "Postal: {n}.");
    }
    if held > 0 {
        let _ = writeln!(
            *summary,
            "Kept the seed point: {held} places whose core point lies outside every admin unit."
        );
    }
    if unnamed > 0 {
        let _ = writeln!(*summary, "Left out: {unnamed} places with no usable name.");
    }
    if abroad > 0 {
        let _ = writeln!(
            *summary,
            "Left out: {abroad} natural places more than 200 m outside every admin unit (D-201)."
        );
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

/// What the build reads from the country profile (and, for search, its lexicon).
#[derive(Debug, Default)]
pub(crate) struct Profile {
    pub(crate) phonetic: Option<PhoneticKey>,
    /// The phonetic key's profile name (`indic-v0`), for the pack's query rules.
    pub(crate) phonetic_name: Option<String>,
    /// Folded.
    pub(crate) droppable: Vec<String>,
    pub(crate) wikidata: Option<String>,
    /// The romanisation profiles of its names (`ne-Deva`), in order.
    pub(crate) romanise: Vec<String>,
    /// The unit types, in profile order: a row's unit type is its position plus 1 (D-120).
    pub(crate) units: Vec<UnitType>,
    /// Per source family (`codab`): where a unit's code holds its unit type.
    pub(crate) unit_codes: BTreeMap<String, CodeSlice>,
    /// Native-script generics, folded, each with its Latin spellings, folded.
    pub(crate) native: Vec<(String, Vec<String>)>,
    /// The country's languages (ISO 639 codes), in profile order: a baseline slice keeps names
    /// in them and English (D-152).
    pub(crate) languages: Vec<String>,
}

/// A unit type of the profile.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct UnitType {
    pub(crate) level: u8,
    #[serde(default)]
    pub(crate) abbreviations: Vec<Term>,
    /// Per source family, the code of this type.
    #[serde(default)]
    pub(crate) source_codes: BTreeMap<String, String>,
}

/// The part of a unit's code that holds its unit type.
#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct CodeSlice {
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

#[derive(serde::Deserialize)]
struct ProfileFile {
    #[serde(default)]
    phonetic_key: Option<String>,
    #[serde(default)]
    droppable_suffixes: Vec<Term>,
    #[serde(default)]
    wikidata: Option<String>,
    #[serde(default)]
    romanise: Vec<String>,
    #[serde(default)]
    unit_types: Vec<UnitType>,
    #[serde(default)]
    sources: BTreeMap<String, SourceRules>,
    #[serde(default)]
    languages: Vec<String>,
}

#[derive(serde::Deserialize)]
struct SourceRules {
    #[serde(default)]
    unit_type_code: Option<CodeSlice>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct Term {
    pub(crate) text: String,
    /// The fold of `text`, set when the profile is read.
    #[serde(skip)]
    pub(crate) folded: String,
}

impl Term {
    pub(crate) fn folded(&self) -> &str {
        &self.folded
    }
}

impl Profile {
    pub(crate) fn read(root: &Path, cc: &str) -> Result<Self, String> {
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
            phonetic_name: p.phonetic_key,
            droppable: p.droppable_suffixes.iter().map(|t| fold(&t.text)).collect(),
            wikidata: p.wikidata,
            romanise: p.romanise,
            units: p
                .unit_types
                .into_iter()
                .map(|mut u| {
                    for a in &mut u.abbreviations {
                        a.folded = fold(&a.text);
                    }
                    u
                })
                .collect(),
            unit_codes: p
                .sources
                .into_iter()
                .filter_map(|(family, r)| r.unit_type_code.map(|c| (family, c)))
                .collect(),
            native: crate::names::native_generics(root, cc)?,
            languages: p.languages,
        })
    }

    /// The unit type of an admin unit (1 + its position in the profile), from the code in its
    /// key (`admin:<code>`) and the rule of the boundary source's family (`codab` for
    /// `codab-np-geojson`); 0 for none.
    pub(crate) fn unit_of(&self, admin_source: &str, key: &str, level: u8) -> u8 {
        let Some(code) = key.strip_prefix("admin:") else {
            return 0;
        };
        let Some((family, slice)) = self.unit_codes.iter().find(|(f, _)| {
            admin_source
                .strip_prefix(f.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        }) else {
            return 0;
        };
        let Some(digit) = code.get(slice.offset..slice.offset + slice.length) else {
            return 0;
        };
        self.units
            .iter()
            .position(|u| {
                u.level == level && u.source_codes.get(family).is_some_and(|c| c == digit)
            })
            .and_then(|i| u8::try_from(i + 1).ok())
            .unwrap_or(0)
    }
}

/// Drops the names that cannot be stored, then the places left without a canonical name (a
/// source record with an empty name); returns how many places were dropped.
fn keep_named(places: &mut Vec<Place>) -> usize {
    for p in places.iter_mut() {
        p.names.retain(|n| pack::fits(&n.text));
    }
    let before = places.len();
    places.retain(|p| !p.names.is_empty());
    before - places.len()
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use super::PlaceName;
    use super::naming::{Here, Naming, place_names};
    use super::sources::Rec;
    use crate::aliases::rules::{Dropped, Rules};
    use crate::names::Namer;
    use teigeodata_names::PhoneticKey;

    fn rec(name: &str) -> Rec {
        Rec {
            kind: "populated_place".into(),
            class: String::new(),
            name: name.into(),
            point: Some((863_000_000, 267_000_000)),
            population: None,
        }
    }

    fn alias(text: &str) -> PlaceName {
        PlaceName {
            text: text.into(),
            lang: None,
            kind: Some(teigeodata_format::names::NameType::Alt),
        }
    }

    /// D-060 in the build: the anchor keeps every name; another record lends its names only
    /// when corroborated, and never one that names an admin unit or a nearby place, unless the
    /// anchor has that name too (a town and the municipality named after it).
    #[test]
    fn names_follow_the_alias_rules() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let namer = Namer::load(&root, "np", &["ne-Deva".into()]).expect("np loads");
        let seed = (863_000_000, 267_000_000);
        let mut rules = Rules::new(PhoneticKey::from_name("indic-v0"), Some(&namer));
        rules.add_admin("Lahan");
        rules.add_admin("Siraha");
        rules.add_place(0, seed, "लहान");
        rules.add_place(1, (863_000_000, 267_450_000), "Golbazar");
        rules.finish();
        let (osm, gn, wd) = (rec("लहान"), rec("Lahan"), rec("नेपाल"));
        let by_source: Vec<BTreeMap<String, Vec<PlaceName>>> = vec![
            BTreeMap::from([("osm:1".into(), vec![alias("Golbazar")])]),
            BTreeMap::from([(
                "geonames:1".into(),
                vec![alias("Siraha"), alias("Golbazar")],
            )]),
            BTreeMap::from([("wikidata:Q1".into(), vec![alias("Nepal")])]),
        ];
        let recs = [
            (&osm, "osm:1", 0),
            (&gn, "geonames:1", 1),
            (&wd, "wikidata:Q1", 2),
        ];
        let here = Here {
            cluster: "c",
            owner: 0,
            seed,
        };
        let mut naming = Naming::default();
        let all = place_names(&here, &recs, &by_source, (&namer, &rules), &mut naming)
            .expect("a name fits");
        let texts: Vec<&str> = all.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(texts.first(), Some(&"Lahan"), "the display name");
        let set: BTreeSet<&str> = texts.into_iter().collect();
        // Golbazar is a place 5 km away, but the anchor names it, so it stays; Lahan names a
        // municipality, but it is the anchor's own name romanised.
        assert_eq!(set, BTreeSet::from(["Golbazar", "Lahan", "लहान"]));
        let dropped: Vec<(&str, &str, Dropped, &str)> = naming
            .dropped
            .iter()
            .map(|(_, r, t, w, with)| (r.as_str(), t.as_str(), *w, with.as_str()))
            .collect();
        assert_eq!(
            dropped,
            [
                ("geonames:1", "Siraha", Dropped::Collision, "admin:Siraha"),
                ("wikidata:Q1", "नेपाल", Dropped::Uncorroborated, ""),
            ]
        );
    }

    #[test]
    fn a_unit_type_comes_from_the_code_by_the_source_family() {
        use super::{CodeSlice, Profile, UnitType};
        let unit = |level: u8, code: &str| UnitType {
            level,
            source_codes: [("codab".to_string(), code.to_string())].into(),
            ..UnitType::default()
        };
        let p = Profile {
            units: vec![unit(3, "1"), unit(3, "4"), unit(2, "4")],
            unit_codes: [(
                "codab".to_string(),
                CodeSlice {
                    offset: 6,
                    length: 1,
                },
            )]
            .into(),
            ..Profile::default()
        };
        assert_eq!(p.unit_of("codab-np-geojson", "admin:NP0329401", 3), 2);
        assert_eq!(p.unit_of("codab-np-geojson", "admin:NP0327101", 3), 1);
        // The level must match; a district code is no local unit type.
        assert_eq!(p.unit_of("codab-np-geojson", "admin:NP0329401", 2), 3);
        assert_eq!(p.unit_of("codab-np-geojson", "admin:NP0329901", 3), 0);
        // Another family, a longer family name, a short code, a non-admin key: none.
        assert_eq!(p.unit_of("geoboundaries-np", "admin:NP0329401", 3), 0);
        assert_eq!(p.unit_of("codabx-np", "admin:NP0329401", 3), 0);
        assert_eq!(p.unit_of("codab-np-geojson", "admin:NP03", 3), 0);
        assert_eq!(p.unit_of("codab-np-geojson", "geonames:1", 3), 0);
    }

    #[test]
    fn a_pack_carries_the_notices_of_what_it_reads() {
        let source = |id: &str, class: &str, notice: &str| {
            format!(
                "[[source]]\nid = \"{id}\"\nkind = \"http\"\nurl = \"https://x/{id}\"\nsha256 = \"00\"\nsize = 1\nlicence_class = \"{class}\"\nattribution = \"{notice}\"\n\n"
            )
        };
        let text = [
            source("osm", "odbl", "© OpenStreetMap contributors, ODbL"),
            source("gn", "cc-by", "GeoNames, CC BY 4.0"),
            source("gn-alt", "cc-by", "GeoNames, CC BY 4.0"),
            source("bounds", "cc-by", "Boundaries, CC BY-IGO"),
            source("ms", "cc-by", "Microsoft buildings, CDLA"),
        ]
        .concat();
        let lock: crate::lock::Lock = toml::from_str(&text).unwrap();
        let root = Path::new("/nonexistent-forge-root");
        let args = |extra: &[&str]| {
            let mut v: Vec<String> = ["xx", "--release", "1.0.0", "--admin", "bounds"]
                .iter()
                .chain(extra)
                .map(ToString::to_string)
                .collect();
            v.extend(["osm", "gn", "gn-alt"].map(String::from));
            super::Args::parse(&v).unwrap()
        };
        // Main: every source read, a shared notice once, in reading order (the core run last).
        let main = super::gate(root, &args(&["--core", "osm+ms"]), &lock).unwrap();
        assert_eq!(
            main.attribution,
            "© OpenStreetMap contributors, ODbL\nGeoNames, CC BY 4.0\nBoundaries, CC BY-IGO\nMicrosoft buildings, CDLA"
        );
        // Lite: the ODbL source is not read, so its notice is not carried.
        let lite = super::gate(root, &args(&["--lite", "--core", "ms"]), &lock).unwrap();
        assert_eq!(
            lite.attribution,
            "GeoNames, CC BY 4.0\nBoundaries, CC BY-IGO\nMicrosoft buildings, CDLA"
        );
        // Lite refuses a core run with a source it may not read.
        let refused = super::gate(root, &args(&["--lite", "--core", "osm+ms"]), &lock);
        assert!(refused.is_err_and(|e| e.contains("core run osm+ms reads osm")));
        // Notices over the manifest's limit are refused.
        let long = "n".repeat(teigeodata_format::MAX_TEXT);
        let big: crate::lock::Lock =
            toml::from_str(&[source("a", "cc-by", &long), source("b", "cc-by", "b")].concat())
                .unwrap();
        assert!(super::notices(&big, &["a", "b"]).is_err());
        assert!(super::notices(&big, &["a"]).is_ok());
    }
}
