//! `forge world`: the world part of the embedded baseline (D-144, D-147).
//!
//! ```text
//! forge world --release <v> [--lite] [--admin <source>] [--admin1 <source>]
//!             [--cities <source>] [--tolerance-m <m>] [--islet-km2 <km²>] [--ids <dir>] [--key <seed-file>]
//! ```
//!
//! Rows: one country place per Natural Earth admin-0 feature (Nepal's point of view) that
//! holds land, its first-level units, and every city of the city list (`GeoNames`
//! `cities15000`), all outside the countries that build a slice of their own pack (a recipe
//! with `baseline:`, D-145). The units come from `--admin1`:
//!
//! - **Lite** (`ne-admin1`, D-150): Natural Earth's admin-1 features, each cut to the country
//!   that holds most of it ([`crate::topo::pov::cut`]); the countries keep Natural Earth's
//!   outlines.
//! - **Main** (`overture-divisions`, D-155): Overture's regions, drawn from OSM, owned by the
//!   point of view ([`super::world_overture`]); each country is its units and fillers
//!   dissolved, so its outline is OSM's too.
//!
//! A city's parent is the unit that contains it, else the feature, on the cut and source
//! polygons, so the point of view decides, not the city's stated country code; a city outside
//! every feature (a coastal point off a coarse coast) takes the feature of its code.
//!
//! IDs: every place is keyed in the namespace of its country code (a feature without an ISO
//! code, a disputed area, in `world`), by `ne:<ADM0_A3>` and `wikidata:` for countries,
//! `ne1:<adm1_code>`, `wikidata:` and `geonames:` for Natural Earth's units,
//! `gers:<division ID>`, `wikidata:` and `osm:relation/<id>` for Overture's, and `geonames:`
//! for cities (a key two
//! rows share stays with the first of country, city, unit), through one ID registry per namespace
//! (`world-ids-<build>/<ns>.tsv`, the next build's `--ids`). A country that builds its own
//! pack is matched in that pack's registry instead, by its Wikidata ID, so the world's row for
//! it carries the pack's ID and the pack's slice answers for it (the ID gate: no match fails).
//!
//! A pack country is drawn by its slice's own outline, its neighbours cut to meet it there and
//! its arcs pinned, so the shipped baseline's borders are one line (D-154, [`pov::redraw`]).
//!
//! Geometry: the `topology` stage's arcs and polygons of the countries and units together
//! (`topo::run_world`: one arc set, each country dissolved from its units and remainders),
//! re-encoded at the
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
use crate::topo::pov;
use teigeodata_format::elevation::HeightSource;

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
    /// Its settlement rank from the feature class and code (`P.PPLC` a national capital,
    /// `P.PPLA` a first-level seat), as a pack's records (`sources::settlement_rank`).
    pub(super) rank: u8,
    /// `elevation` (surveyed), else the `dem` column (SRTM3 or GTOPO30, the model), in
    /// metres, with its source (D-219).
    pub(super) elevation: Option<(i16, HeightSource)>,
    /// The IANA zone the list states (`timezone`), empty when it states none.
    pub(super) timezone: String,
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
            class,
            code,
            cc,
            _,
            _,
            _,
            _,
            _,
            pop,
            elev,
            dem,
            timezone,
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
            rank: super::sources::settlement_rank(&format!("{class}.{code}")),
            elevation: metres(elev)
                .map(|m| (m, HeightSource::Surveyed))
                .or_else(|| metres(dem).map(|m| (m, HeightSource::Model))),
            timezone: timezone.to_string(),
        });
    }
    Ok(out)
}

/// The finest level of the outlines within this many metres by default (D-144: at 1 km world
/// admin 0 is about 182,000 vertices counted per ring).
const TOLERANCE_M: u32 = 1_000;

/// Main's default from Overture (D-157): OSM within 2 km is nearer the ground than Natural
/// Earth within 1 km (0.6–10 km off), and 1 km would not fit Z1 (9.76 MB against 5.55 MB,
/// the research note §3.7).
const TOLERANCE_M_OVERTURE: u32 = 2_000;

/// The cell of the containment index, 1° (a country spans many; the index stays small at
/// world scale).
const CELL_E7: i32 = 10_000_000;

/// Main's first-level units (D-155); the lite build cannot take them (`ODbL`).
const OVERTURE: &str = "overture-divisions";

/// A first-level unit of the world part, from either source.
pub(super) struct WorldUnit {
    /// Its row key: `ne1:<adm1_code>` or `gers:<division ID>`; the topology's code is the part
    /// after the colon.
    pub(super) key: String,
    /// Its other source keys (Wikidata, `GeoNames`, OSM).
    pub(super) refs: Vec<String>,
    /// The untagged source name first, then each language's.
    pub(super) names: Vec<(Option<&'static str>, String)>,
    /// The source's label point, when it gives one.
    pub(super) label: Option<(i32, i32)>,
    /// Its country's index.
    pub(super) country: usize,
    pub(super) rings: Rings,
}

/// A polygon's rings, `(lon, lat)` on the 1e-7° grid.
type Rings = crate::topo::thin::Rings;

impl WorldUnit {
    fn code(&self) -> &str {
        self.key.split_once(':').map_or(self.key.as_str(), |k| k.1)
    }
}

/// The world's land below the countries: units, and fillers (a country's land no unit holds).
pub(super) struct Partition {
    pub(super) units: Vec<WorldUnit>,
    pub(super) fills: Vec<(usize, Rings)>,
    /// The pack countries' indices: their outlines are their slices', pinned (D-154).
    pub(super) pinned: Vec<usize>,
    /// The summary's lines on the units.
    pub(super) report: String,
}

struct Args {
    release: String,
    lite: bool,
    admin: String,
    admin1: String,
    cities: String,
    tz: String,
    tolerance_m: u32,
    /// Overture's islets smaller than this are left out; by default the tolerance's square
    /// (`world_overture::Islets`, D-157).
    islet_km2: f64,
    ids: Option<String>,
    key: Option<String>,
    /// Label sources, a chain (D-221).
    labels: Vec<String>,
}

/// The file-name part of a world part built from `cities` and the label chain `labels`
/// (TG-13, D-221): empty for the default list and no labels, so the default file keeps its name.
pub(crate) fn variant(cities: &str, labels: &[String]) -> String {
    let mut v = String::new();
    if cities != DEFAULT_CITIES {
        v.push('-');
        v.push_str(cities);
    }
    for l in labels {
        v.push('-');
        v.push_str(l);
    }
    v
}

/// The default city list; another (`--cities geonames-cities5000`) writes its own world part,
/// `world-baseline-<build>-<source>.tgd` (TG-13).
pub(crate) const DEFAULT_CITIES: &str = "geonames-cities15000";

fn parse(args: &[String]) -> Result<Args, String> {
    let usage = "usage: forge world --release <v> [--lite] [--admin <source>] [--admin1 <source>] [--cities <source>] [--tz <source>] [--tolerance-m <m>] [--islet-km2 <km²>] [--labels <source>]… [--ids <dir>] [--key <seed-file>]";
    let mut a = Args {
        release: String::new(),
        lite: false,
        admin: "ne-admin0-nep".into(),
        admin1: String::new(),
        cities: DEFAULT_CITIES.into(),
        tz: "tzdb-2026f".into(),
        tolerance_m: 0,
        islet_km2: -1.0,
        ids: None,
        key: None,
        labels: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(f) = it.next() {
        let mut value = || it.next().cloned().ok_or(usage);
        match f.as_str() {
            "--release" => a.release = value()?,
            "--lite" => a.lite = true,
            "--admin" => a.admin = value()?,
            "--admin1" => a.admin1 = value()?,
            "--cities" => a.cities = value()?,
            "--tz" => a.tz = value()?,
            "--tolerance-m" => {
                a.tolerance_m = value()?
                    .parse()
                    .ok()
                    .filter(|&m: &u32| m > 0)
                    .ok_or("--tolerance-m: metres, above 0")?;
            }
            "--islet-km2" => {
                a.islet_km2 = value()?
                    .parse()
                    .ok()
                    .filter(|v: &f64| v.is_finite() && *v >= 0.0)
                    .ok_or("--islet-km2: km², at least 0")?;
            }
            "--ids" => a.ids = Some(value()?),
            "--key" => a.key = Some(value()?),
            "--labels" => a.labels.push(value()?),
            _ => return Err(format!("unknown argument `{f}`; {usage}")),
        }
    }
    if a.release.is_empty() {
        return Err(usage.into());
    }
    if a.admin1.is_empty() {
        a.admin1 = if a.lite { "ne-admin1" } else { OVERTURE }.into();
    }
    if a.tolerance_m == 0 {
        a.tolerance_m = if a.admin1 == OVERTURE {
            TOLERANCE_M_OVERTURE
        } else {
            TOLERANCE_M
        };
    }
    if a.islet_km2 < 0.0 {
        a.islet_km2 = (f64::from(a.tolerance_m) / 1e3).powi(2);
    }
    Ok(a)
}

/// The countries whose recipe builds a baseline slice (D-145): their places come from their
/// pack, and their registry names the world's row for them.
/// A pack country's outline is its slice's, and its neighbours meet it there (D-154): the pack
/// countries' indices (their arcs are pinned) and the report line.
fn redraw_packs(
    root: &Path,
    packs: &[String],
    build: &str,
    countries: &mut [Country],
) -> Result<(Vec<usize>, String), String> {
    let outlines = pack_outlines(root, packs, build, countries)?;
    let mut shapes: Vec<Vec<Vec<(i32, i32)>>> =
        countries.iter().map(|c| c.polygon.rings.clone()).collect();
    let r = pov::redraw(&mut shapes, &outlines);
    for (c, rings) in countries.iter_mut().zip(shapes) {
        c.polygon.rings = rings;
    }
    let report = format!(
        "- Pack outlines (D-154): {} pack countries drawn by their slice, {} neighbours cut by \
         them, {} pieces of Natural Earth's outline given to the neighbour sharing the most \
         border, {} touching none.",
        outlines.len(),
        r.cut,
        r.given,
        r.left
    );
    Ok((outlines.iter().map(|o| o.0).collect(), report))
}

/// Each pack country's index in `countries` and its outline as its baseline slice ships it
/// (the vertices at the slice's level of detail), so the world's neighbours meet it on the
/// slice's own line (D-154).
pub(super) fn pack_outlines(
    root: &Path,
    packs: &[String],
    build: &str,
    countries: &[Country],
) -> Result<Vec<pov::Outline>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let mut out = Vec::new();
    for cc in packs {
        let file = root.join(format!(
            "data/build/baseline/{cc}/{cc}-baseline-{build}.tgd"
        ));
        let bytes = fs::read(&file).map_err(|e| {
            format!(
                "{}: {e} (run `forge make {cc}` first: its slice draws the country)",
                file.display()
            )
        })?;
        let pack = Pack::open_verified(&bytes).map_err(e)?;
        let sec = |k: Kind| pack.section(k).ok_or(format!("{cc}: the slice has no {k}"));
        let places = teigeodata_format::places::Places::open(&sec(Kind::PLACES)?).map_err(e)?;
        let arcs = Arcs::open(&sec(Kind::ARCS)?).map_err(e)?;
        let polys =
            teigeodata_format::polygons::Polygons::open(&sec(Kind::POLYGONS)?).map_err(e)?;
        let row = (0..places.len())
            .find(|&r| places.kind(r) == Some(PlaceKind::Country))
            .ok_or(format!("{cc}: the slice has no country"))?;
        let p = polys
            .polygon_of(u32::try_from(row).map_err(|_| "too many rows")?)
            .ok_or(format!("{cc}: the slice's country has no outline"))?;
        let mut rings = Vec::new();
        for part in polys.parts(p).ok_or(format!("{cc}: an outline's parts"))? {
            for ring in polys.rings(part).ok_or(format!("{cc}: a part's rings"))? {
                rings.push(
                    polys
                        .ring(&arcs, ring, 0)
                        .map(|v| (v.lon(), v.lat()))
                        .collect(),
                );
            }
        }
        let i = countries
            .iter()
            .position(|c| c.iso2.as_deref() == Some(cc.as_str()))
            .ok_or(format!("{cc}: no Natural Earth country has this code"))?;
        out.push((i, rings));
    }
    Ok(out)
}

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
    elevation: Option<(i16, HeightSource)>,
    /// The zone the source states (a city's), and the key of the zone row assigned (D-151).
    stated_tz: Option<String>,
    zone: Option<String>,
}

fn namespace(cc: Option<&str>) -> Namespace {
    cc.and_then(|c| <[u8; 2]>::try_from(c.as_bytes()).ok())
        .and_then(Namespace::from_country)
        .unwrap_or(Namespace::WORLD)
}

/// The languages a first-level unit keeps besides its source name (D-150): English, the
/// API's fallback. All 26 of Natural Earth's would be 3.5 MB of the 8 MB budget (NAMES and
/// SEARCH, measured 2026-11-10); a country's pack carries its units' full names.
const UNIT_LANGS: [&str; 1] = ["en"];

/// A unit's untagged source name and its [`UNIT_LANGS`] names.
pub(super) fn unit_names(
    names: &[(Option<&'static str>, String)],
) -> Vec<(Option<&'static str>, String)> {
    names
        .iter()
        .filter(|(lang, _)| lang.is_none_or(|l| UNIT_LANGS.contains(&l)))
        .cloned()
        .collect()
}

/// A feature's names: `NAME` first (the canonical name), then each language's, without
/// repeating a text already listed in the same language or untagged; names too long to store
/// left out.
fn names_of(names: &[(Option<&'static str>, String)]) -> Vec<PlaceName> {
    let mut seen: BTreeSet<(Option<&str>, &str)> = BTreeSet::new();
    let mut out = Vec::new();
    // A name too long to store is left out first, as a pack's are (`keep_named`), so the
    // canonical name is the first one stored (Overture's bilingual Abkhazia, D-155).
    let storable = names.iter().filter(|(_, t)| super::pack::fits(t));
    for (i, (lang, text)) in storable.enumerate() {
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
                    names: names_of(&c.names),
                    population: c.population,
                    rank: 0,
                    parent: None,
                    sources,
                },
                ns: namespace(c.iso2.as_deref()),
                elevation: None,
                stated_tz: None,
                zone: None,
            })
        })
        .collect()
}

/// The unit rows: each unit under its country, at the source's label point when it is inside
/// the unit, else the topology's.
fn unit_rows(
    units: &[WorldUnit],
    countries: &[Country],
    labels: &BTreeMap<&str, (i32, i32)>,
) -> Result<Vec<Row>, String> {
    units
        .iter()
        .map(|u| {
            let c = countries
                .get(u.country)
                .ok_or_else(|| format!("{}: a country out of range", u.key))?;
            let inside = Polygon {
                rings: u.rings.clone(),
            };
            let point = u
                .label
                .filter(|&(x, y)| inside.contains(x, y))
                .or_else(|| labels.get(u.code()).copied())
                .ok_or_else(|| format!("{}: no label point", u.key))?;
            let mut sources = vec![u.key.clone()];
            sources.extend(u.refs.iter().cloned());
            Ok(Row {
                place: Place {
                    key: u.key.clone(),
                    kind: PlaceKind::Admin,
                    admin_level: 1,
                    flags: 0,
                    point,
                    names: names_of(&u.names),
                    population: None,
                    rank: 0,
                    parent: Some(format!("ne:{}", c.code)),
                    sources,
                },
                ns: namespace(c.iso2.as_deref()),
                elevation: None,
                stated_tz: None,
                zone: None,
            })
        })
        .collect()
}

/// Drops every source key a row of higher priority (country, then city, then unit) already
/// holds, so keys are unique across rows as the registries need; a row's own key (its first)
/// is never dropped. Returns how many were dropped.
fn dedup_keys(rows: &mut [Row]) -> usize {
    let rank = |k: PlaceKind| match k {
        PlaceKind::Country => 0,
        PlaceKind::Settlement => 1,
        _ => 2,
    };
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| {
        rows.get(i)
            .map(|r| (rank(r.place.kind), r.place.key.clone()))
    });
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut dropped = 0;
    for i in order {
        let Some(r) = rows.get_mut(i) else { continue };
        let own = r.place.key.clone();
        let before = r.place.sources.len();
        r.place.sources.retain(|k| *k == own || !seen.contains(k));
        dropped += before - r.place.sources.len();
        seen.extend(r.place.sources.iter().cloned());
    }
    dropped
}

/// The city rows outside `skip`, each under the unit that contains it, else the feature. Returns them and how
/// many fell back to the feature of their code, and how many have no parent at all.
fn city_rows(
    cities: Vec<City>,
    countries: &[Country],
    units: &[WorldUnit],
    skip: &BTreeSet<String>,
) -> (Vec<Row>, usize, usize) {
    let polys: Vec<Polygon> = countries.iter().map(|c| c.polygon.clone()).collect();
    let index = GridIndex::new(&polys, CELL_E7);
    let pieces: Vec<Polygon> = units
        .iter()
        .map(|u| Polygon {
            rings: u.rings.clone(),
        })
        .collect();
    let piece_index = GridIndex::new(&pieces, CELL_E7);
    let unit_of = |x: i32, y: i32| {
        piece_index
            .candidates(x, y)
            .find(|&i| pieces.get(i).is_some_and(|p| p.contains(x, y)))
            .and_then(|i| units.get(i))
            .map(|u| u.key.clone())
    };
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
                    rank: c.rank,
                    parent: unit_of(c.point.0, c.point.1).or_else(|| {
                        parent
                            .and_then(|i| countries.get(i))
                            .map(|p| format!("ne:{}", p.code))
                    }),
                },
                ns: namespace(Some(&c.cc)),
                elevation: c.elevation,
                stated_tz: Some(c.timezone),
                zone: None,
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

/// Main's units and outlines from Overture (D-155), or Natural Earth's cut to the view
/// (lite, and main with `--admin1 ne-admin1`).
fn partition(
    root: &Path,
    a: &Args,
    lock: &Lock,
    packs: &[String],
    build: &str,
    countries: &mut Vec<Country>,
) -> Result<Partition, String> {
    if a.admin1 == OVERTURE {
        let points: Vec<_> = read_cities(&lock.path(root, &a.cities)?)?
            .iter()
            .map(|c| c.point)
            .collect();
        let islets = super::world_overture::Islets {
            min_km2: a.islet_km2,
            keep: &points,
        };
        super::world_overture::partition(root, lock, &a.admin1, packs, build, countries, &islets)
    } else {
        ne_partition(root, lock, &a.admin1, packs, build, countries)
    }
}

/// `forge world …`.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let a = parse(args)?;
    let build = if a.lite { "lite" } else { "main" };
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    for id in [&a.admin, &a.admin1, &a.cities, &a.tz]
        .into_iter()
        .chain(&a.labels)
    {
        let s = lock.get(id)?;
        if a.lite && !super::LITE.contains(&s.licence_class.as_str()) {
            return Err(format!(
                "{id}: licence class {} is not allowed in the lite build",
                s.licence_class
            ));
        }
    }
    let packs = pack_countries(root)?;
    let mut countries = natural_earth::read(&lock.path(root, &a.admin)?)?;
    let part = partition(root, &a, &lock, &packs, build, &mut countries)?;
    let (rows, geometry, counts) = world_rows(root, &a, &packs, &lock, &countries, &part)?;

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
    let read: Vec<&str> = [&a.admin, &a.admin1, &a.cities, &a.tz]
        .into_iter()
        .chain(&a.labels)
        .map(String::as_str)
        .collect();
    let notices = super::notices(&lock, &read)?;
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
    // A city list other than the default is its own world part, named by it (TG-13).
    let variant = variant(&a.cities, &a.labels);
    let file = dir.join(format!("world-baseline-{build}{variant}.tgd"));
    crate::make::write_atomic(&file, &out)?;
    let probes = probes(&countries, &ids, &row_of);
    crate::make::write_atomic(
        &dir.join(format!("world-probes-{build}{variant}.tsv")),
        probes.as_bytes(),
    )?;
    let mut r = format!(
        "# World baseline ({build})\n\n{} rows outside {}; {} namespaces.\n\n{counts}\n\n\
         | Section | Bytes |\n|---|---:|\n",
        places.len(),
        if packs.is_empty() {
            "the pack countries (none)".to_string()
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
    fs::write(dir.join(format!("summary-{build}{variant}.md")), &r).map_err(|e| e.to_string())?;
    Ok(format!(
        "{r}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(&file).display(),
        t.elapsed()
    ))
}

/// Natural Earth's admin-1 units cut to the countries (D-150), the pack countries redrawn by
/// their slices first (D-154).
fn ne_partition(
    root: &Path,
    lock: &Lock,
    source: &str,
    packs: &[String],
    build: &str,
    countries: &mut [Country],
) -> Result<Partition, String> {
    let units = natural_earth::read_admin1(&lock.path(root, source)?)?;
    let (pinned, redrawn) = redraw_packs(root, packs, build, countries)?;
    let c_in: Vec<pov::Input<'_>> = countries
        .iter()
        .map(|c| pov::Input {
            code: &c.code,
            rings: &c.polygon.rings,
        })
        .collect();
    let u_in: Vec<pov::Input<'_>> = units
        .iter()
        .map(|u| pov::Input {
            code: &u.adm0,
            rings: &u.polygon.rings,
        })
        .collect();
    // Units the view does not recognise, and unnamed placeholders, are not places: their area
    // falls to their country's remainder.
    let unrecognized: Vec<bool> = units
        .iter()
        .map(|u| u.unrecognized || u.names.is_empty())
        .collect();
    let unnamed = units.iter().filter(|u| u.names.is_empty()).count();
    // A pack country's units come from its slice.
    let cut = pov::cut(
        &c_in,
        &u_in,
        &unrecognized,
        |c| {
            countries
                .get(c)
                .and_then(|c| c.iso2.as_ref())
                .is_none_or(|cc| !packs.contains(cc))
        },
        &pinned,
    );
    let moved: Vec<&str> = cut
        .moved
        .iter()
        .filter_map(|&i| units.get(i).map(|u| u.code.as_str()))
        .collect();
    let report = format!(
        "- {} first-level units ({source}): {} cut to the country that holds most of them, {} \
         in a country other than their code's ({}), {} not recognised in this point of view \
         or unnamed placeholders ({unnamed} of them), {} off every coast, {} of pack countries \
         left to their slice; {} remainder pieces.\n{redrawn}",
        cut.pieces.len(),
        cut.clipped,
        moved.len(),
        moved.join(", "),
        cut.unrecognized,
        cut.outside,
        cut.skipped,
        cut.fills.len(),
    );
    let world_units = cut
        .pieces
        .into_iter()
        .map(|p| {
            let u = units.get(p.unit).ok_or("a piece out of range")?;
            let mut refs: Vec<String> =
                u.wikidata.iter().map(|q| format!("wikidata:{q}")).collect();
            refs.extend(u.geonames.map(|g| format!("geonames:{g}")));
            Ok(WorldUnit {
                key: format!("ne1:{}", u.code),
                refs,
                names: unit_names(&u.names),
                label: u.label,
                country: p.country,
                rings: p.rings,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Partition {
        units: world_units,
        fills: cut.fills,
        pinned,
        report,
    })
}

/// The rows (countries, units, cities, keys unique) and the outlines' geometry, with a report.
/// The topology runs every time from the pinned sources (countries and units together), so it
/// is never stale.
fn world_rows(
    root: &Path,
    a: &Args,
    packs: &[String],
    lock: &Lock,
    countries: &[Country],
    part: &Partition,
) -> Result<(Vec<Row>, super::geometry::Geometry, String), String> {
    let name = format!("{}+{}", a.admin, a.admin1);
    let units: Vec<crate::topo::UnitRef<'_>> = part
        .units
        .iter()
        .map(|u| (u.code(), u.country, u.rings.as_slice()))
        .collect();
    crate::topo::run_world(root, &name, countries, &units, &part.fills, &part.pinned)?;
    let geometry = super::geometry::load(root, &name)?;
    let labels = geometry.labels();
    let mut rows = country_rows(countries, &labels)?;
    rows.extend(unit_rows(&part.units, countries, &labels)?);
    let skip: BTreeSet<String> = packs.iter().cloned().collect();
    let (cities, fallback, orphans) = city_rows(
        read_cities(&lock.path(root, &a.cities)?)?,
        countries,
        &part.units,
        &skip,
    );
    let n_cities = cities.len();
    let under_unit = cities
        .iter()
        .filter(|c| {
            c.place
                .parent
                .as_deref()
                .is_some_and(|p| !p.starts_with("ne:"))
        })
        .count();
    rows.extend(cities);
    let dropped = dedup_keys(&mut rows);
    let chain = super::world_labels::read(root, lock, &a.labels)?;
    let named = super::world_labels::apply(&chain, rows.iter_mut().map(|r| &mut r.place));
    let tz_report = zone_rows(root, a, lock, countries, &mut rows)?;
    let counts = format!(
        "- {} countries and disputed areas ({}).\n{}\n- {n_cities} cities ({}): {under_unit} \
         under a unit, {fallback} under the feature of their code, {orphans} with no \
         country.\n- {dropped} source keys another row already holds dropped.\n{named}{tz_report}",
        countries.len(),
        a.admin,
        part.report,
        a.cities,
    );
    Ok((rows, geometry, counts))
}

/// About this many border probes are written (D-149).
const PROBES: usize = 100_000;

/// The locator's border probes (D-149): every k-th vertex of every feature's source outline,
/// at full resolution, with the ID of its feature, k chosen for about [`PROBES`] in all.
/// `lat lon id` per line, the coordinates on the 1e-7° grid and the ID raw: the engine's
/// locator gate asks the stored, simplified outlines for the countries near each probe and
/// expects its feature among them.
fn probes(countries: &[Country], ids: &[PlaceId], row_of: &BTreeMap<&str, u32>) -> String {
    let total: usize = countries
        .iter()
        .flat_map(|c| &c.polygon.rings)
        .map(Vec::len)
        .sum();
    let stride = (total / PROBES).max(1);
    let mut out = format!("# lat_e7\tlon_e7\tid: every {stride}th of {total} source vertices\n");
    let mut n = 0_usize;
    for c in countries {
        let Some(id) = row_of
            .get(format!("ne:{}", c.code).as_str())
            .and_then(|&r| ids.get(r as usize))
        else {
            continue;
        };
        for &(lon, lat) in c.polygon.rings.iter().flatten() {
            if n.is_multiple_of(stride) {
                let _ = writeln!(out, "{lat}\t{lon}\t{}", id.get());
            }
            n += 1;
        }
    }
    out
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
    let (names, search, _) = name_sections(places, &Profile::default(), "")?;
    let heights: Vec<Option<i16>> = rows.iter().map(|r| r.elevation.map(|e| e.0)).collect();
    let sources: Vec<Option<HeightSource>> =
        rows.iter().map(|r| r.elevation.map(|e| e.1)).collect();
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
    if let Some(body) = teigeodata_format::elevation::encode_with(&heights, &sources).map_err(e)? {
        full.push((Kind::ELEVATION, Encoding::TABLE, n, body));
    }
    let by_code: BTreeMap<String, u32> = row_of
        .iter()
        .filter_map(|(k, &r)| {
            k.strip_prefix("ne:")
                .or_else(|| k.strip_prefix("ne1:"))
                .or_else(|| k.strip_prefix("gers:"))
                .map(|c| (c.to_string(), r))
        })
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
    let mut out = extract::rows(&pack, &all, lod, &super::baseline::region_options()).map_err(e)?;
    // Each place's zone row (D-151); `extract` keeps every row in order.
    let zone_of = zone_rows_of(rows, row_of)?;
    if let Some(body) = teigeodata_format::place_tz::encode(&zone_of).map_err(e)? {
        out.push((Kind::PLACE_TZ, Encoding::TABLE, n, body));
    }
    Ok(out)
}

/// Each row's zone as the PLACES row of its zone place.
fn zone_rows_of(rows: &[&Row], row_of: &BTreeMap<&str, u32>) -> Result<Vec<Option<u32>>, String> {
    rows.iter()
        .map(|r| {
            r.zone
                .as_deref()
                .map(|z| row_of.get(z).copied().ok_or(format!("{z}: no row")))
                .transpose()
        })
        .collect()
}

/// The largest share of cities' people whose own zone differs from the zone of the unit or
/// country they hang from, so where `tz_at` differs from it (G-tz for the world part, D-151;
/// measured 2026-11-10 in the decision).
const MAX_TZ_OFF: f64 = 0.02;

/// Assigns every row its zone ([`super::world_tz`]) and adds a row per zone used: a timezone
/// area keyed `tz:<ID>` in the `world` namespace, at its `zone.tab` point. G-tz fails past
/// [`MAX_TZ_OFF`].
fn zone_rows(
    root: &Path,
    a: &Args,
    lock: &Lock,
    countries: &[Country],
    rows: &mut Vec<Row>,
) -> Result<String, String> {
    use super::world_tz::{Item, Tzdb, assign};
    let tarball = lock.path(root, &a.tz)?;
    let tz = Tzdb::parse(
        &super::zones::tar_file(&tarball, "zone.tab")?,
        &super::zones::tar_file(&tarball, "backward")?,
    )?;
    let iso2: BTreeMap<String, &str> = countries
        .iter()
        .filter_map(|c| Some((format!("ne:{}", c.code), c.iso2.as_deref()?)))
        .collect();
    let items: Vec<Item<'_>> = rows
        .iter()
        .map(|r| Item {
            key: &r.place.key,
            kind: r.place.kind,
            parent: r.place.parent.as_deref(),
            iso2: iso2.get(&r.place.key).copied(),
            point: r.place.point,
            population: r.place.population,
            stated: r.stated_tz.as_deref(),
        })
        .collect();
    let assigned = assign(&items, &tz)?;
    drop(items);
    if assigned.off_share > MAX_TZ_OFF {
        return Err(format!(
            "G-tz: {:.3}% of cities' people state a zone other than their unit's (limit {:.1}%)\n{}",
            assigned.off_share * 100.0,
            MAX_TZ_OFF * 100.0,
            assigned.report
        ));
    }
    for (r, z) in rows.iter_mut().zip(&assigned.of) {
        r.zone = z
            .and_then(|z| assigned.zones.get(z))
            .map(|(id, _)| format!("tz:{id}"));
    }
    for (id, point) in &assigned.zones {
        let key = format!("tz:{id}");
        rows.push(Row {
            place: Place {
                sources: vec![key.clone()],
                key,
                kind: PlaceKind::TimezoneArea,
                admin_level: 0,
                flags: 0,
                point: *point,
                names: vec![PlaceName {
                    text: id.clone(),
                    lang: None,
                    kind: Some(NameType::Official),
                }],
                population: None,
                rank: 0,
                parent: None,
            },
            ns: Namespace::WORLD,
            elevation: None,
            stated_tz: None,
            zone: None,
        });
    }
    Ok(assigned.report)
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
