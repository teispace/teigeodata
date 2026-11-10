//! `forge world`: the world part of the embedded baseline (D-144, D-147).
//!
//! ```text
//! forge world --release <v> [--lite] [--admin <source>] [--cities <source>]
//!             [--tolerance-m <m>] [--ids <dir>] [--key <seed-file>]
//! ```
//!
//! Rows: one country place per Natural Earth admin-0 feature (Nepal's point of view), and every
//! city of the city list (`GeoNames` `cities15000`) outside the countries that build a slice
//! of their own pack (a recipe with `baseline:`, D-145). A city's parent is the feature that
//! contains it on the source polygons, so the point of view decides, not the city's stated
//! country code; a city outside every feature (a coastal point off a coarse coast) takes the
//! feature of its code.
//!
//! IDs: every place is keyed in the namespace of its country code (a feature without an ISO
//! code, a disputed area, in `world`), by `ne:<ADM0_A3>` and `wikidata:` for countries and
//! `geonames:` for cities, through one ID registry per namespace
//! (`world-ids-<build>/<ns>.tsv`, the next build's `--ids`). A country that builds its own
//! pack is matched in that pack's registry instead, by its Wikidata ID, so the world's row for
//! it carries the pack's ID and the pack's slice answers for it (the ID gate: no match fails).
//!
//! Geometry: the `topology` stage's arcs and polygons of the admin source, re-encoded at the
//! finest level within the tolerance with the baseline's region index
//! (`teigeodata_format::extract`). Output: `data/build/baseline/world/`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use teigeodata_core::{Namespace, PlaceId, PlaceKind, hilbert};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::names::NameType;
use teigeodata_format::{
    Encoding, Kind, Pack, Tag, Writer, extract, hierarchy, idmap, place_index,
};

use super::ids::{Registry, State, assign};
use super::pack::{NODE, e6, name_sections, places_section};
use super::{Place, PlaceName, Profile};
use crate::geom::{GridIndex, Polygon};
use crate::lock::Lock;
use crate::read::natural_earth::{self, Country};

/// One row of the city list (`GeoNames`' dump columns).
#[derive(Debug)]
pub(super) struct City {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) ascii: String,
    /// (lon, lat) on the 1e-7° grid.
    pub(super) point: (i32, i32),
    /// The ISO 3166-1 code, lower case.
    pub(super) cc: String,
    pub(super) population: Option<i64>,
    /// `elevation`, else the `dem` column (SRTM3 or GTOPO30), in metres.
    pub(super) elevation: Option<i16>,
}

/// Every row of the city list: the `.txt` table inside its zip.
pub(super) fn read_cities(zip: &Path) -> Result<Vec<City>, String> {
    let file = fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let name = archive
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.ends_with(".txt") && n != "readme.txt")
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| format!("{}: no city table", zip.display()))?;
    let entry = archive
        .by_name(&name)
        .map_err(|e| format!("{}: {name}: {e}", zip.display()))?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(entry).lines().enumerate() {
        let line = line.map_err(|e| format!("{name}: {e}"))?;
        // geonameid, name, asciiname, alternatenames, lat, lon, class, code, country, cc2,
        // admin1–4, population, elevation, dem, timezone, modified
        let f: Vec<&str> = line.split('\t').collect();
        let bad = || format!("{name} line {}: malformed", i + 1);
        let [
            id,
            n,
            ascii,
            _,
            lat,
            lon,
            _,
            _,
            cc,
            _,
            _,
            _,
            _,
            _,
            pop,
            elev,
            dem,
            ..,
        ] = f[..]
        else {
            return Err(bad());
        };
        let deg = |v: &str| v.parse::<f64>().ok().and_then(crate::record::e7);
        let metres = |v: &str| v.parse::<i16>().ok().filter(|&m| m > -1000);
        out.push(City {
            id: id.parse().map_err(|_| bad())?,
            name: n.to_string(),
            ascii: ascii.to_string(),
            point: (deg(lon).ok_or_else(bad)?, deg(lat).ok_or_else(bad)?),
            cc: cc.to_ascii_lowercase(),
            population: pop.parse().ok().filter(|&p: &i64| p > 0),
            elevation: metres(elev).or_else(|| metres(dem)),
        });
    }
    Ok(out)
}

/// The finest level of the outlines within this many metres by default (D-144: at 1 km world
/// admin 0 is about 182,000 vertices counted per ring).
const TOLERANCE_M: u32 = 1_000;

/// The cell of the containment index, 1° (a country spans many; the index stays small at
/// world scale).
const CELL_E7: i32 = 10_000_000;

struct Args {
    release: String,
    lite: bool,
    admin: String,
    cities: String,
    tolerance_m: u32,
    ids: Option<String>,
    key: Option<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let usage = "usage: forge world --release <v> [--lite] [--admin <source>] [--cities <source>] [--tolerance-m <m>] [--ids <dir>] [--key <seed-file>]";
    let mut a = Args {
        release: String::new(),
        lite: false,
        admin: "ne-admin0-nep".into(),
        cities: "geonames-cities15000".into(),
        tolerance_m: TOLERANCE_M,
        ids: None,
        key: None,
    };
    let mut it = args.iter();
    while let Some(f) = it.next() {
        let mut value = || it.next().cloned().ok_or(usage);
        match f.as_str() {
            "--release" => a.release = value()?,
            "--lite" => a.lite = true,
            "--admin" => a.admin = value()?,
            "--cities" => a.cities = value()?,
            "--tolerance-m" => {
                a.tolerance_m = value()?.parse().map_err(|_| "--tolerance-m: metres")?;
            }
            "--ids" => a.ids = Some(value()?),
            "--key" => a.key = Some(value()?),
            _ => return Err(format!("unknown argument `{f}`; {usage}")),
        }
    }
    if a.release.is_empty() {
        return Err(usage.into());
    }
    Ok(a)
}

/// The countries whose recipe builds a baseline slice (D-145): their places come from their
/// pack, and their registry names the world's row for them.
fn pack_countries(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("data/countries");
    let mut out = Vec::new();
    for e in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.join("build.yaml").is_file()
            && crate::make::Recipe::read(&p)?.baseline.is_some()
            && let Some(cc) = p.file_name().and_then(|n| n.to_str())
        {
            out.push(cc.to_string());
        }
    }
    out.sort();
    Ok(out)
}

/// A place and the namespace it is keyed in.
struct Row {
    place: Place,
    ns: Namespace,
    elevation: Option<i16>,
}

fn namespace(cc: Option<&str>) -> Namespace {
    cc.and_then(|c| <[u8; 2]>::try_from(c.as_bytes()).ok())
        .and_then(Namespace::from_country)
        .unwrap_or(Namespace::WORLD)
}

/// A country's names: `NAME` first (the canonical name), then each language's, without
/// repeating a text already listed in the same language or untagged.
fn country_names(c: &Country) -> Vec<PlaceName> {
    let mut seen: BTreeSet<(Option<&str>, &str)> = BTreeSet::new();
    let mut out = Vec::new();
    for (i, (lang, text)) in c.names.iter().enumerate() {
        if !seen.insert((*lang, text)) || (lang.is_some() && seen.contains(&(None, text.as_str())))
        {
            continue;
        }
        out.push(PlaceName {
            text: text.clone(),
            lang: lang.map(str::to_string),
            kind: (i == 0).then_some(NameType::Official),
        });
    }
    out
}

/// The country rows, and the index of each feature's row by `ADM0_A3`.
fn country_rows(
    countries: &[Country],
    labels: &BTreeMap<&str, (i32, i32)>,
) -> Result<Vec<Row>, String> {
    countries
        .iter()
        .map(|c| {
            // The cartographers' label when it is inside the feature, else the topology's.
            let point = c
                .label
                .filter(|&(x, y)| c.polygon.contains(x, y))
                .or_else(|| labels.get(c.code.as_str()).copied())
                .ok_or_else(|| format!("{}: no label point", c.code))?;
            let mut sources = vec![format!("ne:{}", c.code)];
            sources.extend(c.wikidata.as_ref().map(|q| format!("wikidata:{q}")));
            Ok(Row {
                place: Place {
                    key: format!("ne:{}", c.code),
                    kind: PlaceKind::Country,
                    admin_level: 0,
                    flags: 0,
                    point,
                    names: country_names(c),
                    population: c.population,
                    rank: 0,
                    parent: None,
                    sources,
                },
                ns: namespace(c.iso2.as_deref()),
                elevation: None,
            })
        })
        .collect()
}

/// The city rows outside `skip`, each under the feature that contains it. Returns them and how
/// many fell back to the feature of their code, and how many have no parent at all.
fn city_rows(
    cities: Vec<City>,
    countries: &[Country],
    skip: &BTreeSet<String>,
) -> (Vec<Row>, usize, usize) {
    let polys: Vec<Polygon> = countries.iter().map(|c| c.polygon.clone()).collect();
    let index = GridIndex::new(&polys, CELL_E7);
    // The feature of each code: the largest by box, for codes held by several (France and its
    // overseas parts).
    let mut by_code: BTreeMap<&str, (i64, usize)> = BTreeMap::new();
    for (i, c) in countries.iter().enumerate() {
        if let (Some(cc), Some(b)) = (c.iso2.as_deref(), c.polygon.bbox()) {
            let area = (i64::from(b.2) - i64::from(b.0)) * (i64::from(b.3) - i64::from(b.1));
            let e = by_code.entry(cc).or_insert((area, i));
            if area > e.0 {
                *e = (area, i);
            }
        }
    }
    let parents: Vec<(Option<usize>, bool)> = cities
        .par_iter()
        .map(|c| {
            let (x, y) = c.point;
            let inside = index
                .candidates(x, y)
                .find(|&i| polys.get(i).is_some_and(|p| p.contains(x, y)));
            match inside {
                Some(i) => (Some(i), false),
                None => (by_code.get(c.cc.as_str()).map(|e| e.1), true),
            }
        })
        .collect();
    let (mut fallback, mut orphans) = (0, 0);
    let rows = cities
        .into_iter()
        .zip(parents)
        .filter(|(c, _)| !skip.contains(&c.cc))
        .map(|(c, (parent, fell_back))| {
            fallback += usize::from(fell_back && parent.is_some());
            orphans += usize::from(parent.is_none());
            let mut names = vec![PlaceName {
                text: c.name.clone(),
                lang: None,
                kind: None,
            }];
            if c.ascii != c.name && !c.ascii.is_empty() {
                names.push(PlaceName {
                    text: c.ascii.clone(),
                    lang: None,
                    kind: Some(NameType::Alt),
                });
            }
            let key = format!("geonames:{}", c.id);
            Row {
                place: Place {
                    sources: vec![key.clone()],
                    key,
                    kind: PlaceKind::Settlement,
                    admin_level: 0,
                    flags: 0,
                    point: c.point,
                    names,
                    population: c.population,
                    rank: 0,
                    parent: parent
                        .and_then(|i| countries.get(i))
                        .map(|p| format!("ne:{}", p.code)),
                },
                ns: namespace(Some(&c.cc)),
                elevation: c.elevation,
            }
        })
        .collect();
    (rows, fallback, orphans)
}

/// The live serial of `build` holding `key`, if any.
fn live_serial(reg: &Registry, key: &str, build: &str) -> Option<u64> {
    reg.entries.iter().find_map(|(&s, e)| {
        (matches!(&e.state, State::Live(b) if b.contains(build)) && e.keys.contains(key))
            .then_some(s)
    })
}

fn read_registry(path: &Path) -> Result<Registry, String> {
    match fs::read_to_string(path) {
        Ok(text) => Registry::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// IDs for `rows` (in row order): a pack country's row from its pack's registry (by Wikidata
/// ID), every other row through its namespace's world registry. Returns the IDs and the
/// updated world registries by namespace.
fn allocate(
    root: &Path,
    rows: &[&Row],
    a: &Args,
    packs: &[String],
) -> Result<(Vec<PlaceId>, BTreeMap<String, Registry>), String> {
    let build = if a.lite { "lite" } else { "main" };
    let mut ids = vec![None; rows.len()];
    let mut groups: BTreeMap<Namespace, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        let cc =
            r.ns.country()
                .map(|c| String::from_utf8_lossy(&c).into_owned());
        if r.place.kind == PlaceKind::Country
            && let Some(cc) = cc.filter(|c| packs.contains(c))
        {
            let file = root.join(format!("data/build/pack/{cc}/{cc}-ids-{build}.tsv"));
            let reg = read_registry(&file)?;
            let serial = r
                .place
                .sources
                .iter()
                .filter(|k| k.starts_with("wikidata:"))
                .find_map(|k| live_serial(&reg, k, build))
                .ok_or_else(|| {
                    format!(
                        "{}: no live serial in {} holds its Wikidata ID (ID gate; run `forge make {cc}` first)",
                        r.place.key,
                        file.display()
                    )
                })?;
            if let Some(slot) = ids.get_mut(i) {
                *slot = PlaceId::new(r.ns, serial);
            }
            continue;
        }
        groups.entry(r.ns).or_default().push(i);
    }
    let mut registries = BTreeMap::new();
    for (ns, members) in groups {
        let name = ns.country().map_or_else(
            || "world".to_string(),
            |c| String::from_utf8_lossy(&c).into_owned(),
        );
        let old = match &a.ids {
            Some(dir) => read_registry(&root.join(dir).join(format!("{name}.tsv")))?,
            None => Registry::default(),
        };
        let keys: Vec<Vec<String>> = members
            .iter()
            .filter_map(|&i| rows.get(i).map(|r| r.place.sources.clone()))
            .collect();
        let assigned = assign(&old, &keys, build)?;
        for (&i, &s) in members.iter().zip(&assigned.serials) {
            if let Some(slot) = ids.get_mut(i) {
                *slot = PlaceId::new(ns, s);
            }
        }
        registries.insert(name, assigned.registry);
    }
    let ids = ids
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or("a serial out of range")?;
    Ok((ids, registries))
}

/// `forge world …`.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let a = parse(args)?;
    let build = if a.lite { "lite" } else { "main" };
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    for id in [&a.admin, &a.cities] {
        let s = lock.get(id)?;
        if a.lite && !super::LITE.contains(&s.licence_class.as_str()) {
            return Err(format!(
                "{id}: licence class {} is not allowed in the lite build",
                s.licence_class
            ));
        }
    }
    let packs = pack_countries(root)?;
    let countries = natural_earth::read(&lock.path(root, &a.admin)?)?;
    // The outlines' topology, always from the pinned source (7 s for the world, so it is
    // never stale).
    crate::topo::run(root, &a.admin)?;
    let geometry = super::geometry::load(root, &a.admin)?;
    let labels = geometry.labels();
    let mut rows = country_rows(&countries, &labels)?;
    let skip: BTreeSet<String> = packs.iter().cloned().collect();
    let (cities, fallback, orphans) = city_rows(
        read_cities(&lock.path(root, &a.cities)?)?,
        &countries,
        &skip,
    );
    let n_cities = cities.len();
    rows.extend(cities);

    // Rows: Hilbert order of the stored point, ties by key (as a country pack).
    let mut order: Vec<(u64, &str, usize)> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| Ok((hilbert(e6(r.place.point)?), r.place.key.as_str(), i)))
        .collect::<Result<_, String>>()?;
    order.sort_unstable();
    let rows: Vec<&Row> = order.iter().filter_map(|o| rows.get(o.2)).collect();
    let places: Vec<&Place> = rows.iter().map(|r| &r.place).collect();
    let row_of: BTreeMap<&str, u32> = places
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.as_str(), u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    if row_of.len() != places.len() {
        return Err("two places share a key".into());
    }
    let (ids, registries) = allocate(root, &rows, &a, &packs)?;
    let sections = sections(&places, &rows, &ids, &row_of, &geometry, a.tolerance_m)?;
    let notices = super::notices(&lock, &[a.admin.as_str(), a.cities.as_str()])?;
    let out = write(&sections, &a, &notices, root)?;
    super::baseline::check(&out, places.len())?;
    verify(&out, &places)?;

    let dir = root.join("data/build/baseline/world");
    let ids_dir = dir.join(format!("world-ids-{build}"));
    fs::create_dir_all(&ids_dir).map_err(|e| format!("{}: {e}", ids_dir.display()))?;
    for (name, reg) in &registries {
        crate::make::write_atomic(
            &ids_dir.join(format!("{name}.tsv")),
            reg.render().as_bytes(),
        )?;
    }
    let file = dir.join(format!("world-baseline-{build}.tgd"));
    crate::make::write_atomic(&file, &out)?;
    let mut r = format!(
        "# World baseline ({build})\n\n{} rows: {} countries and disputed areas ({}), {n_cities} \
         cities ({}) outside {} ({fallback} under the feature of their code, {orphans} with \
         no country); {} namespaces.\n\n| Section | Bytes |\n|---|---:|\n",
        places.len(),
        countries.len(),
        a.admin,
        a.cities,
        if packs.is_empty() {
            "no pack country".to_string()
        } else {
            packs.join(", ")
        },
        registries.len(),
    );
    for s in &sections {
        let _ = writeln!(r, "| {} | {} |", s.0, s.3.len());
    }
    let _ = writeln!(
        r,
        "| **pack** | **{}** |\n\nBLAKE3 of the pack: `{}`.",
        out.len(),
        blake3::hash(&out).to_hex()
    );
    fs::write(dir.join(format!("summary-{build}.md")), &r).map_err(|e| e.to_string())?;
    Ok(format!(
        "{r}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(&file).display(),
        t.elapsed()
    ))
}

/// Every section of the world part: the rows' sections as a country pack's, the polygons
/// linked by code, then the geometry re-encoded at the finest level within `tolerance_m`.
fn sections(
    places: &[&Place],
    rows: &[&Row],
    ids: &[PlaceId],
    row_of: &BTreeMap<&str, u32>,
    geometry: &super::geometry::Geometry,
    tolerance_m: u32,
) -> Result<Vec<extract::Body>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let n = places.len() as u64;
    let points: Vec<_> = places
        .iter()
        .map(|p| e6(p.point))
        .collect::<Result<_, _>>()?;
    let parents: Vec<Option<u32>> = places
        .iter()
        .map(|p| p.parent.as_deref().and_then(|k| row_of.get(k).copied()))
        .collect();
    let (names, search) = name_sections(places, &Profile::default(), "")?;
    let heights: Vec<Option<i16>> = rows.iter().map(|r| r.elevation).collect();
    let mut full: Vec<extract::Body> = vec![
        (
            Kind::IDMAP,
            Encoding::TABLE,
            n,
            idmap::encode(ids).map_err(e)?,
        ),
        (
            Kind::PLACES,
            Encoding::TABLE,
            n,
            places_section(places, &points)?,
        ),
        (
            Kind::HIERARCHY,
            Encoding::TABLE,
            n,
            hierarchy::encode(&parents).map_err(e)?,
        ),
        (Kind::NAMES, Encoding::TABLE, n, names),
        (Kind::SEARCH, Encoding::TABLE, n, search),
        (
            Kind::PLACE_INDEX,
            Encoding::TABLE,
            n,
            place_index::encode(&points, NODE).map_err(e)?,
        ),
    ];
    if let Some(body) = teigeodata_format::elevation::encode(&heights).map_err(e)? {
        full.push((Kind::ELEVATION, Encoding::TABLE, n, body));
    }
    let by_code: BTreeMap<String, u32> = row_of
        .iter()
        .filter_map(|(k, &r)| k.strip_prefix("ne:").map(|c| (c.to_string(), r)))
        .collect();
    full.extend(super::geometry::sections_by_code(geometry, &by_code)?);
    // The whole pack in memory, then its rows at the outlines' level (one re-encoding path).
    let refs: Vec<(Kind, Encoding, u64, &[u8])> = full
        .iter()
        .map(|s| (s.0, s.1, s.2, s.3.as_slice()))
        .collect();
    let bytes = Writer::new("world", "baseline", "0.0.0")
        .map_err(e)?
        .pack(&refs, None)
        .map_err(e)?;
    let pack = Pack::open(&bytes).map_err(e)?;
    let arcs = Arcs::open(&pack.section(Kind::ARCS).ok_or("no ARCS")?).map_err(e)?;
    let lod = extract::lod_within(&arcs, tolerance_m.saturating_mul(100));
    let all: Vec<u32> = (0..u32::try_from(n).map_err(|_| "too many rows")?).collect();
    extract::rows(&pack, &all, lod, &super::baseline::region_options()).map_err(e)
}

/// The world part as a pack: address `world`, component `baseline`, the sources' notices,
/// signed with `--key` when given.
fn write(
    sections: &[extract::Body],
    a: &Args,
    notices: &str,
    root: &Path,
) -> Result<Vec<u8>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let mut w = Writer::new("world", "baseline", &a.release).map_err(e)?;
    w.manifest(Tag::ATTRIBUTION, notices.as_bytes())
        .map_err(e)?;
    // ADR-0002, as every pack: the main build is ODbL, the lite build CC BY 4.0.
    let licence = if a.lite { "cc-by-4.0" } else { "odbl" };
    w.manifest(Tag::LICENCE, licence.as_bytes()).map_err(e)?;
    let key = a
        .key
        .as_ref()
        .map(|p| super::pack::signing_key(&root.join(p)))
        .transpose()?;
    let signer = key.as_ref().map(teigeodata_format::signer);
    let bodies: Vec<(Kind, Encoding, u64, &[u8])> = sections
        .iter()
        .map(|s| (s.0, s.1, s.2, s.3.as_slice()))
        .collect();
    w.pack(
        &bodies,
        signer.as_ref().map(|s| s as &teigeodata_format::Sign<'_>),
    )
    .map_err(e)
}

/// Every place keeps its ID, point, parent and canonical name, and is found by its name.
fn verify(bytes: &[u8], places: &[&Place]) -> Result<(), String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let p = Pack::open(bytes).map_err(e)?;
    let sec = |k: Kind| p.section(k).ok_or(format!("no {k}"));
    let pl = teigeodata_format::places::Places::open(&sec(Kind::PLACES)?).map_err(e)?;
    super::pack::verify_rows(
        places,
        &pl,
        &hierarchy::Hierarchy::open(&sec(Kind::HIERARCHY)?).map_err(e)?,
        &idmap::IdMap::open(&sec(Kind::IDMAP)?).map_err(e)?,
        &teigeodata_format::names::Names::open(&sec(Kind::NAMES)?).map_err(e)?,
        &teigeodata_format::search::Search::open(&sec(Kind::SEARCH)?).map_err(e)?,
        &place_index::PlaceIndex::open(&sec(Kind::PLACE_INDEX)?, pl).map_err(e)?,
    )
}
