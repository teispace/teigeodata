//! Admin boundaries from a zipped `GeoJSON` set with one file per level, named
//! `…admin<level>.geojson` (OCHA COD-AB). Properties `adm<level>_pcode` and `adm<level>_name`
//! name each unit; `_em` variants (generalised for maps) are skipped.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use serde::Deserialize;

use crate::geom::Polygon;
use crate::record::e7;

/// One admin unit.
#[derive(Debug)]
pub(crate) struct Unit {
    pub(crate) pcode: String,
    pub(crate) name: String,
    pub(crate) polygon: Polygon,
    /// The P-codes of its ancestors, level 0 first (`adm0_pcode` … `adm<level − 1>_pcode`).
    pub(crate) parents: Vec<String>,
    /// The area the source states, km² (`area_sqkm`), if any.
    pub(crate) area_km2: Option<f64>,
}

#[derive(Deserialize)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    properties: serde_json::Map<String, serde_json::Value>,
    geometry: Option<Geometry>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Geometry {
    Polygon {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    MultiPolygon {
        coordinates: Vec<Vec<Vec<[f64; 2]>>>,
    },
}

/// The admin levels present in a boundary set, ascending.
pub(crate) fn levels(zip_path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut levels: Vec<u8> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| !n.contains("_em"))
        .filter_map(|n| {
            n.strip_suffix(".geojson")?
                .rsplit_once("admin")?
                .1
                .parse()
                .ok()
        })
        .collect();
    levels.sort_unstable();
    levels.dedup();
    Ok(levels)
}

/// Reads the units of one level.
pub(crate) fn read(zip_path: &Path, level: u8) -> Result<Vec<Unit>, String> {
    let file = File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let suffix = format!("admin{level}.geojson");
    let entry = archive
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.ends_with(&suffix) && !n.contains("_em"))
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| format!("{}: no {suffix}", zip_path.display()))?;
    let reader = archive.by_name(&entry).map_err(|e| e.to_string())?;
    let c: Collection = serde_json::from_reader(BufReader::with_capacity(1 << 20, reader))
        .map_err(|e| format!("{entry}: {e}"))?;
    let prop = |f: &Feature, k: &str| {
        f.properties
            .get(&format!("adm{level}_{k}"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let ring = |r: &[[f64; 2]]| -> Vec<(i32, i32)> {
        r.iter()
            .filter_map(|[lon, lat]| Some((e7(*lon)?, e7(*lat)?)))
            .collect()
    };
    let mut units: Vec<Unit> = c
        .features
        .iter()
        .map(|f| {
            // A multipolygon's parts become rings of one even-odd polygon: parts never
            // overlap within a unit, so the parity is the same.
            let rings = match &f.geometry {
                Some(Geometry::Polygon { coordinates }) => {
                    coordinates.iter().map(|r| ring(r)).collect()
                }
                Some(Geometry::MultiPolygon { coordinates }) => {
                    coordinates.iter().flatten().map(|r| ring(r)).collect()
                }
                None => Vec::new(),
            };
            let parents = (0..level)
                .map(|l| {
                    f.properties
                        .get(&format!("adm{l}_pcode"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                })
                .collect();
            Unit {
                pcode: prop(f, "pcode"),
                name: prop(f, "name"),
                polygon: Polygon { rings },
                parents,
                area_km2: f
                    .properties
                    .get("area_sqkm")
                    .and_then(serde_json::Value::as_f64),
            }
        })
        .collect();
    units.sort_by(|a, b| a.pcode.cmp(&b.pcode));
    Ok(units)
}
