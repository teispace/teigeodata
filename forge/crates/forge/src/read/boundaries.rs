//! Admin boundaries from a zipped `GeoJSON` set with one file per level, named
//! `…admin<level>.geojson` (OCHA COD-AB). Properties `adm<level>_pcode` and `adm<level>_name`
//! name each unit; `_em` variants (generalised for maps) are skipped. A geoBoundaries level
//! (one plain `GeoJSON` file) is read by [`read_geoboundaries`].

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
    let mut units: Vec<Unit> = c
        .features
        .iter()
        .map(|f| {
            let rings = rings(f);
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

/// A feature's rings on the 1e-7° grid. A multipolygon's parts become rings of one even-odd
/// polygon: parts never overlap within a unit, so the parity is the same.
fn rings(f: &Feature) -> Vec<Vec<(i32, i32)>> {
    let ring = |r: &[[f64; 2]]| -> Vec<(i32, i32)> {
        r.iter()
            .filter_map(|[lon, lat]| Some((e7(*lon)?, e7(*lat)?)))
            .collect()
    };
    match &f.geometry {
        Some(Geometry::Polygon { coordinates }) => coordinates.iter().map(|r| ring(r)).collect(),
        Some(Geometry::MultiPolygon { coordinates }) => {
            coordinates.iter().flatten().map(|r| ring(r)).collect()
        }
        None => Vec::new(),
    }
}

/// Reads one geoBoundaries level from its plain `GeoJSON` file (`geoBoundaries-<ISO3>-ADM<n>`):
/// `shapeType` gives the level, `shapeID` the code, `shapeName` the name and `shapeGroup` (the
/// ISO 3166-1 alpha-3 code) the only ancestor the file states, level 0. Returns the level and
/// its units, sorted by code.
pub(crate) fn read_geoboundaries(path: &Path) -> Result<(u8, Vec<Unit>), String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let c: Collection = serde_json::from_reader(BufReader::with_capacity(1 << 20, file))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let prop = |f: &Feature, k: &str| {
        f.properties
            .get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut level = None;
    let mut units = Vec::with_capacity(c.features.len());
    for f in &c.features {
        let kind = prop(f, "shapeType");
        let l: u8 = kind
            .strip_prefix("ADM")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| format!("{}: shapeType {kind:?} is not ADM<n>", path.display()))?;
        if *level.get_or_insert(l) != l {
            return Err(format!("{}: more than one level", path.display()));
        }
        let pcode = prop(f, "shapeID");
        if pcode.is_empty() {
            return Err(format!("{}: a feature has no shapeID", path.display()));
        }
        let mut parents = vec![String::new(); usize::from(l)];
        if let Some(p) = parents.first_mut() {
            *p = prop(f, "shapeGroup");
        }
        units.push(Unit {
            pcode,
            name: prop(f, "shapeName"),
            polygon: Polygon { rings: rings(f) },
            parents,
            area_km2: None,
        });
    }
    units.sort_by(|a, b| a.pcode.cmp(&b.pcode));
    Ok((
        level.ok_or_else(|| format!("{}: no features", path.display()))?,
        units,
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use std::fs;

    use super::*;

    fn file(name: &str, json: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("forge-gb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, json).unwrap();
        p
    }

    const SQUARE: &str =
        r#"{"type":"Polygon","coordinates":[[[85,27],[86,27],[86,28],[85,28],[85,27]]]}"#;

    #[test]
    fn a_geoboundaries_level_is_read() {
        let p = file(
            "ok.geojson",
            &format!(
                r#"{{"type":"FeatureCollection","features":[
                {{"properties":{{"shapeName":"B","shapeID":"ID2","shapeGroup":"NPL","shapeType":"ADM2"}},"geometry":{SQUARE}}},
                {{"properties":{{"shapeName":"A","shapeID":"ID1","shapeGroup":"NPL","shapeType":"ADM2"}},"geometry":{SQUARE}}}]}}"#
            ),
        );
        let (level, units) = read_geoboundaries(&p).unwrap();
        assert_eq!(level, 2);
        assert_eq!(
            units.iter().map(|u| u.pcode.as_str()).collect::<Vec<_>>(),
            ["ID1", "ID2"]
        );
        assert_eq!(units[0].name, "A");
        assert_eq!(units[0].parents, ["NPL", ""]);
        assert_eq!(units[0].polygon.rings[0][1], (860_000_000, 270_000_000));
    }

    #[test]
    fn mixed_levels_and_missing_ids_are_refused() {
        let feature = |id: &str, kind: &str| {
            format!(
                r#"{{"properties":{{"shapeID":"{id}","shapeGroup":"NPL","shapeType":"{kind}"}},"geometry":{SQUARE}}}"#
            )
        };
        let collection = |f: &[String]| {
            format!(
                r#"{{"type":"FeatureCollection","features":[{}]}}"#,
                f.join(",")
            )
        };
        for (name, body) in [
            (
                "mixed.geojson",
                collection(&[feature("a", "ADM1"), feature("b", "ADM2")]),
            ),
            ("noid.geojson", collection(&[feature("", "ADM1")])),
            ("kind.geojson", collection(&[feature("a", "ADMX")])),
            ("empty.geojson", collection(&[])),
        ] {
            assert!(read_geoboundaries(&file(name, &body)).is_err(), "{name}");
        }
    }
}
