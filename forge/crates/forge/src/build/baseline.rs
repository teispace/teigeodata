//! `forge baseline <cc>`: a country pack's slice of the embedded baseline (D-144, D-145).
//!
//! The slice keeps, from the country's built pack: the country, its current level-1 units, and
//! every city of the baseline's city list (`GeoNames` `cities15000`) in that country, found by
//! its `geonames:` key in the build's ID registry. So its IDs, names and outline are the pack's,
//! and Nepal's outline is the main pack's 2020 map. Each city's parent becomes its nearest
//! kept ancestor (its level-1 unit), and the admin geometry is the pack's own simplification at
//! the finest level within the tolerance (`teigeodata_format::extract`).
//!
//! G-base: every listed city of the country must be in the pack, or the build fails, naming
//! them. The slice is read back and its geometry checked before it is written.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use teigeodata_core::{Namespace, PlaceId, PlaceKind};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::places::{self, Places};
use teigeodata_format::polygons::Polygons;
use teigeodata_format::region::Region;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer, extract};

use super::ids::{Registry, State};
use crate::lock::Lock;

/// The default simplification tolerance of the baseline's outlines, in metres (D-144: 500 m
/// to 1 km keeps world admin 1 within its share of the 8 MB budget).
const TOLERANCE_M: u32 = 500;

/// The slice's `ADMIN_REGION` index (D-145): a 4,096-cell fine grid instead of the packs' 2^18.
/// On Nepal's slice at 500 m (2026-11-09) it is 20,516 B instead of 135,832 B, and `admin_at`
/// takes 281 ns instead of 163 ns: a fallback tier does not need the packs' speed.
pub(super) fn region_options() -> teigeodata_format::region::Options {
    teigeodata_format::region::Options {
        fine_cells: 1 << 12,
        ..teigeodata_format::region::Options::default()
    }
}

struct Args {
    cc: String,
    lite: bool,
    tolerance_m: u32,
    cities: String,
    key: Option<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let usage = "usage: forge baseline <cc> [--lite] [--tolerance-m <metres>] [--cities <source>] [--key <seed-file>]";
    let mut it = args.iter();
    let cc = it.next().ok_or(usage)?.clone();
    let mut a = Args {
        cc,
        lite: false,
        tolerance_m: TOLERANCE_M,
        cities: "geonames-cities15000".into(),
        key: None,
    };
    while let Some(f) = it.next() {
        let mut value = || it.next().cloned().ok_or(usage);
        match f.as_str() {
            "--lite" => a.lite = true,
            "--tolerance-m" => {
                a.tolerance_m = value()?.parse().map_err(|_| "--tolerance-m: metres")?;
            }
            "--cities" => a.cities = value()?,
            "--key" => a.key = Some(value()?),
            _ => return Err(format!("unknown argument `{f}`; {usage}")),
        }
    }
    Ok(a)
}

/// The `GeoNames` IDs and names of the listed cities in country `cc` (lower case).
fn listed_cities(zip: &Path, cc: &str) -> Result<BTreeMap<u64, String>, String> {
    Ok(super::world::read_cities(zip)?
        .into_iter()
        .filter(|c| c.cc == cc)
        .map(|c| (c.id, c.name))
        .collect())
}

/// The live serial of each `geonames:` key in `build`'s packs.
fn geonames_serials(reg: &Registry, build: &str) -> BTreeMap<u64, u64> {
    let mut out = BTreeMap::new();
    for (&serial, e) in &reg.entries {
        if !matches!(&e.state, State::Live(b) if b.contains(build)) {
            continue;
        }
        for k in &e.keys {
            if let Some(id) = k.strip_prefix("geonames:").and_then(|v| v.parse().ok()) {
                out.insert(id, serial);
            }
        }
    }
    out
}

/// The rows a slice keeps, and what they are.
struct Selection {
    keep: Vec<u32>,
    countries: usize,
    units: usize,
    listed: usize,
    cities: usize,
    /// Listed cities the build merged into one place (D-116), each group joined by ` = `.
    shared: Vec<String>,
}

/// The country, its current level-1 units and the listed cities (G-base: all of them).
fn select(
    pack: &Pack<'_>,
    listed: &BTreeMap<u64, String>,
    serial_of: &BTreeMap<u64, u64>,
    (cc, build): (&str, &str),
) -> Result<Selection, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let ids = IdMap::open(&pack.section(Kind::IDMAP).ok_or("no IDMAP")?).map_err(e)?;
    let places = Places::open(&pack.section(Kind::PLACES).ok_or("no PLACES")?).map_err(e)?;
    let code: [u8; 2] = cc
        .as_bytes()
        .try_into()
        .map_err(|_| format!("`{cc}` is not a two-letter country code"))?;
    let ns = Namespace::from_country(code).ok_or_else(|| format!("no namespace for `{cc}`"))?;
    let row32 = |r: usize| u32::try_from(r).map_err(|_| "too many rows".to_string());
    let mut keep = BTreeSet::new();
    let mut missing = Vec::new();
    let mut named: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (&gid, name) in listed {
        let row = serial_of
            .get(&gid)
            .and_then(|&s| PlaceId::new(ns, s))
            .and_then(|id| ids.row(id));
        match row {
            Some(r) => {
                keep.insert(row32(r)?);
                named
                    .entry(r)
                    .or_default()
                    .push(format!("{name} (geonames:{gid})"));
            }
            None => missing.push(format!("{name} (geonames:{gid})")),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "G-base: {} of {} listed cities are not in the {build} pack: {}",
            missing.len(),
            listed.len(),
            missing.join(", ")
        ));
    }
    let cities = keep.len();
    let (mut countries, mut units) = (0, 0);
    for r in 0..places.len() {
        let current = places.flags(r).unwrap_or(0) & places::FLAG_FORMER == 0;
        match places.kind(r) {
            Some(PlaceKind::Country) => countries += 1,
            Some(PlaceKind::Admin) if current && places.admin_level(r) == Some(1) => units += 1,
            _ => continue,
        }
        keep.insert(row32(r)?);
    }
    Ok(Selection {
        keep: keep.into_iter().collect(),
        countries,
        units,
        listed: listed.len(),
        cities,
        shared: named
            .into_values()
            .filter(|v| v.len() > 1)
            .map(|v| v.join(" = "))
            .collect(),
    })
}

/// `forge baseline …`.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let a = parse(args)?;
    let build = if a.lite { "lite" } else { "main" };
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let source = lock.get(&a.cities)?;
    if a.lite && !super::LITE.contains(&source.licence_class.as_str()) {
        return Err(format!(
            "{}: licence class {} is not allowed in the lite build",
            a.cities, source.licence_class
        ));
    }
    let dir = root.join("data/build/pack").join(&a.cc);
    let pack_file = dir.join(format!("{}-places-{build}.tgd", a.cc));
    let bytes = fs::read(&pack_file).map_err(|e| {
        format!(
            "{}: {e} (run `forge make {}` first)",
            pack_file.display(),
            a.cc
        )
    })?;
    let ids_file = dir.join(format!("{}-ids-{build}.tsv", a.cc));
    let reg = Registry::parse(
        &fs::read_to_string(&ids_file).map_err(|e| format!("{}: {e}", ids_file.display()))?,
    )
    .map_err(|e| format!("{}: {e}", ids_file.display()))?;
    let listed = listed_cities(&lock.path(root, &a.cities)?, &a.cc.to_ascii_lowercase())?;
    let pack = Pack::open(&bytes).map_err(|e| e.to_string())?;
    let sel = select(
        &pack,
        &listed,
        &geonames_serials(&reg, build),
        (&a.cc, build),
    )?;
    let (lod, tolerance_cm) = match pack.section(Kind::ARCS) {
        Some(s) => {
            let full = Arcs::open(&s).map_err(|e| e.to_string())?;
            let lod = extract::lod_within(&full, a.tolerance_m.saturating_mul(100));
            (lod, full.tolerance_cm(lod).unwrap_or(0))
        }
        None => (0, 0),
    };
    let sections =
        extract::rows(&pack, &sel.keep, lod, &region_options()).map_err(|e| e.to_string())?;
    let out = write(&pack, &sections, &a, root)?;
    check(&out, sel.keep.len())?;
    // Its own directory: the pack directory's stamps own every `-<build>.` file there.
    let out_dir = root.join("data/build/baseline").join(&a.cc);
    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let file = out_dir.join(format!("{}-baseline-{build}.tgd", a.cc));
    crate::make::write_atomic(&file, &out)?;
    let report = report(&a, &sel, (lod, tolerance_cm), &sections, &out);
    fs::write(out_dir.join(format!("summary-{build}.md")), &report).map_err(|e| e.to_string())?;
    Ok(format!(
        "{report}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(&file).display(),
        t.elapsed()
    ))
}

/// The summary: what was kept, the level of the outlines, and each section's bytes.
fn report(
    a: &Args,
    sel: &Selection,
    (lod, tolerance_cm): (u8, u32),
    sections: &[extract::Body],
    out: &[u8],
) -> String {
    let build = if a.lite { "lite" } else { "main" };
    let mut r = format!(
        "# Baseline slice: {} ({build})\n\n{} rows: {} country, {} level-1 units, {} listed \
         cities as {} places ({}); outlines at level {lod} ({} m, asked {} m).\n\n\
         | Section | Bytes |\n|---|---:|\n",
        a.cc,
        sel.keep.len(),
        sel.countries,
        sel.units,
        sel.listed,
        sel.cities,
        a.cities,
        tolerance_cm / 100,
        a.tolerance_m
    );
    for s in sections {
        let _ = writeln!(r, "| {} | {} |", s.0, s.3.len());
    }
    let _ = writeln!(
        r,
        "| **pack** | **{}** |\n\nBLAKE3 of the pack: `{}`.",
        out.len(),
        blake3::hash(out).to_hex()
    );
    if !sel.shared.is_empty() {
        let _ = writeln!(
            r,
            "\nListed cities that are one place: {}.",
            sel.shared.join("; ")
        );
    }
    r
}

/// The slice as a pack: the source pack's address, release, box, licence and attribution, the
/// component `baseline`, signed with `--key` when given.
fn write(
    pack: &Pack<'_>,
    sections: &[extract::Body],
    a: &Args,
    root: &Path,
) -> Result<Vec<u8>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let m = pack.manifest();
    let mut w = Writer::new(m.address(), "baseline", m.release()).map_err(e)?;
    for tag in [Tag::BBOX, Tag::LICENCE, Tag::ATTRIBUTION] {
        if let Some(v) = m.get(tag) {
            w.manifest(tag, v).map_err(e)?;
        }
    }
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

/// Reads the slice back: every section hashes, and the geometry passes the format's checks.
pub(super) fn check(bytes: &[u8], rows: usize) -> Result<(), String> {
    let e = |e: teigeodata_format::Error| format!("the baseline slice: {e}");
    let p = Pack::open_verified(bytes).map_err(e)?;
    if let (Some(a), Some(polys)) = (p.section(Kind::ARCS), p.section(Kind::POLYGONS)) {
        let arcs = Arcs::open(&a).map_err(e)?;
        arcs.check().map_err(e)?;
        Polygons::open(&polys)
            .map_err(e)?
            .check(&arcs, rows)
            .map_err(e)?;
        let admin = p
            .section(Kind::ADMIN_REGION)
            .ok_or("the slice has no ADMIN_REGION")?;
        Region::open_on_arcs(&admin, &arcs)
            .map_err(e)?
            .check()
            .map_err(e)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Registry, geonames_serials};

    #[test]
    fn only_live_serials_of_the_build_name_a_city() {
        let reg = Registry::parse(
            "# teigeodata id registry 1\n\
             1\tlite,main\tgeonames:10 osm:node/1\n\
             2\t>1\tgeonames:20\n\
             3\tx\tgeonames:30\n\
             4\tlite\tgeonames:40 wikidata:Q1\n",
        )
        .unwrap_or_default();
        let main = geonames_serials(&reg, "main");
        assert_eq!(main.into_iter().collect::<Vec<_>>(), vec![(10, 1)]);
        let lite = geonames_serials(&reg, "lite");
        assert_eq!(lite.into_iter().collect::<Vec<_>>(), vec![(10, 1), (40, 4)]);
    }
}
