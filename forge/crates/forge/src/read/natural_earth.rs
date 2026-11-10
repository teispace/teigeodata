//! Natural Earth admin 0 from its plain `GeoJSON` file (`ne_10m_admin_0_countries_<pov>.geojson`,
//! D-144): one feature per country or disputed area. `ADM0_A3` is the unique code,
//! `ISO_A2_EH` the ISO 3166-1 code where one applies (`-99` where none does), `WIKIDATAID` the
//! Wikidata item, `NAME` the short English name, `NAME_<XX>` the name in language `xx`,
//! `LABEL_X`/`LABEL_Y` the cartographers' label point and `POP_EST` the population estimate.
//!
//! Natural Earth admin 1 (`ne_10m_admin_1_states_provinces.geojson`, D-150) is read by
//! [`read_admin1`]: the same layout in lower case (`adm1_code` unique, `adm0_a3` the country it
//! is drawn under in the default point of view, `name_<xx>`, `wikidataid`, `gn_id`,
//! `longitude`/`latitude`), and `FCLASS_NP`, Nepal's point of view's class for the unit
//! (`Unrecognized` for a unit that view does not recognise).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::Deserialize;

use crate::geom::Polygon;
use crate::record::e7;

/// The `NAME_<XX>` columns (lower case in admin 1) and their BCP 47 tags, in column order.
pub(crate) const LANGS: [(&str, &str); 26] = [
    ("NAME_AR", "ar"),
    ("NAME_BN", "bn"),
    ("NAME_DE", "de"),
    ("NAME_EL", "el"),
    ("NAME_EN", "en"),
    ("NAME_ES", "es"),
    ("NAME_FA", "fa"),
    ("NAME_FR", "fr"),
    ("NAME_HE", "he"),
    ("NAME_HI", "hi"),
    ("NAME_HU", "hu"),
    ("NAME_ID", "id"),
    ("NAME_IT", "it"),
    ("NAME_JA", "ja"),
    ("NAME_KO", "ko"),
    ("NAME_NL", "nl"),
    ("NAME_PL", "pl"),
    ("NAME_PT", "pt"),
    ("NAME_RU", "ru"),
    ("NAME_SV", "sv"),
    ("NAME_TR", "tr"),
    ("NAME_UK", "uk"),
    ("NAME_UR", "ur"),
    ("NAME_VI", "vi"),
    ("NAME_ZH", "zh-Hans"),
    ("NAME_ZHT", "zh-Hant"),
];

/// One admin-0 feature.
#[derive(Debug)]
pub(crate) struct Country {
    /// `ADM0_A3`, unique in the file.
    pub(crate) code: String,
    /// `ISO_A2_EH`, lower case, when it is two letters.
    pub(crate) iso2: Option<String>,
    /// `WIKIDATAID` (`Q837`), when set.
    pub(crate) wikidata: Option<String>,
    /// `NAME` first, then each `NAME_<XX>` with its language (duplicates kept: the caller
    /// folds).
    pub(crate) names: Vec<(Option<&'static str>, String)>,
    /// (lon, lat) on the 1e-7° grid.
    pub(crate) label: Option<(i32, i32)>,
    pub(crate) population: Option<i64>,
    pub(crate) polygon: Polygon,
}

/// One admin-1 feature.
#[derive(Debug)]
pub(crate) struct Unit {
    /// `adm1_code`, unique in the file.
    pub(crate) code: String,
    /// `adm0_a3`: the country it is drawn under in the default point of view.
    pub(crate) adm0: String,
    pub(crate) wikidata: Option<String>,
    /// `gn_id`, the `GeoNames` ID of the unit, when positive.
    pub(crate) geonames: Option<u64>,
    /// `name` first, then each `name_<xx>` with its language; empty for a placeholder.
    pub(crate) names: Vec<(Option<&'static str>, String)>,
    /// (lon, lat) on the 1e-7° grid, from `longitude`/`latitude`.
    pub(crate) label: Option<(i32, i32)>,
    /// `FCLASS_NP` is `Unrecognized`: Nepal's point of view does not recognise the unit.
    pub(crate) unrecognized: bool,
    pub(crate) polygon: Polygon,
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

/// Whether `path` is a plain `GeoJSON` file rather than a zip (the COD-AB sets are zipped).
pub(crate) fn is_plain(path: &Path) -> Result<bool, String> {
    let mut magic = [0_u8; 2];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(&magic != b"PK")
}

fn collection(path: &Path) -> Result<Collection, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_reader(BufReader::with_capacity(1 << 20, file))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// A trimmed, non-empty text property.
fn text(f: &Feature, k: &str) -> Option<String> {
    f.properties
        .get(k)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn num(f: &Feature, k: &str) -> Option<f64> {
    f.properties.get(k).and_then(serde_json::Value::as_f64)
}

/// The names: `name` first (untagged), then each language's column; the column names in upper
/// case (admin 0) or lower case (admin 1).
fn names(f: &Feature, upper: bool) -> Vec<(Option<&'static str>, String)> {
    let col = |c: &str| {
        if upper {
            c.to_string()
        } else {
            c.to_ascii_lowercase()
        }
    };
    let mut names = Vec::with_capacity(LANGS.len() + 1);
    names.extend(text(f, &col("NAME")).map(|n| (None, n)));
    for (c, lang) in LANGS {
        names.extend(text(f, &col(c)).map(|n| (Some(lang), n)));
    }
    names
}

/// A point from two degree columns.
fn point(f: &Feature, x: &str, y: &str) -> Option<(i32, i32)> {
    Some((num(f, x).and_then(e7)?, num(f, y).and_then(e7)?))
}

/// Every feature of the file, sorted by `ADM0_A3`.
pub(crate) fn read(path: &Path) -> Result<Vec<Country>, String> {
    let c = collection(path)?;
    let mut out = Vec::with_capacity(c.features.len());
    for f in &c.features {
        let text = |k: &str| text(f, k);
        let num = |k: &str| num(f, k);
        let code = text("ADM0_A3")
            .ok_or_else(|| format!("{}: a feature without ADM0_A3", path.display()))?;
        let names = names(f, true);
        if names.is_empty() {
            return Err(format!("{code}: no name"));
        }
        let label = point(f, "LABEL_X", "LABEL_Y");
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a population estimate is a whole number far below 2^53"
        )]
        let population = num("POP_EST").filter(|p| *p > 0.0).map(|p| p as i64);
        out.push(Country {
            iso2: text("ISO_A2_EH")
                .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
                .map(|c| c.to_ascii_lowercase()),
            wikidata: text("WIKIDATAID").filter(|q| q.starts_with('Q')),
            names,
            label,
            population,
            polygon: Polygon { rings: rings(f) },
            code,
        });
    }
    out.sort_by(|a, b| a.code.cmp(&b.code));
    if let Some(code) = out.windows(2).find_map(|w| match w {
        [a, b] if a.code == b.code => Some(a.code.clone()),
        _ => None,
    }) {
        return Err(format!("{}: ADM0_A3 {code} twice", path.display()));
    }
    Ok(out)
}

/// Every admin-1 feature of the file, sorted by `adm1_code`.
pub(crate) fn read_admin1(path: &Path) -> Result<Vec<Unit>, String> {
    let c = collection(path)?;
    let mut out = Vec::with_capacity(c.features.len());
    for f in &c.features {
        let code = text(f, "adm1_code")
            .ok_or_else(|| format!("{}: a feature without adm1_code", path.display()))?;
        let adm0 = text(f, "adm0_a3").ok_or_else(|| format!("{code}: no adm0_a3"))?;
        // Empty for Natural Earth's unnamed placeholders (`+99?` codes, "minor island").
        let names = names(f, false);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a GeoNames ID is a positive whole number far below 2^53"
        )]
        let geonames = num(f, "gn_id").filter(|g| *g >= 1.0).map(|g| g as u64);
        out.push(Unit {
            adm0,
            wikidata: text(f, "wikidataid").filter(|q| q.starts_with('Q')),
            geonames,
            names,
            label: point(f, "longitude", "latitude"),
            unrecognized: text(f, "FCLASS_NP").as_deref() == Some("Unrecognized"),
            polygon: Polygon { rings: rings(f) },
            code,
        });
    }
    out.sort_by(|a, b| a.code.cmp(&b.code));
    if let Some(code) = out.windows(2).find_map(|w| match w {
        [a, b] if a.code == b.code => Some(a.code.clone()),
        _ => None,
    }) {
        return Err(format!("{}: adm1_code {code} twice", path.display()));
    }
    Ok(out)
}

/// A feature's rings on the 1e-7° grid; a multipolygon's parts become rings of one even-odd
/// polygon, as in [`super::boundaries`].
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    #[test]
    fn a_feature_with_its_names_codes_and_rings() {
        let dir = std::env::temp_dir().join(format!("forge-ne-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ne.geojson");
        std::fs::write(
            &p,
            r#"{"type":"FeatureCollection","features":[
              {"type":"Feature","properties":{"ADM0_A3":"ZZB","ISO_A2_EH":"-99","NAME":"B","WIKIDATAID":""},
               "geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}},
              {"type":"Feature","properties":{"ADM0_A3":"ZZA","ISO_A2_EH":"ZA","NAME":"A","NAME_DE":"A-de","NAME_ZHT":"甲",
                "WIKIDATAID":"Q1","LABEL_X":0.5,"LABEL_Y":0.25,"POP_EST":1000.0},
               "geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]],[[[2,2],[3,2],[3,3],[2,2]]]]}}]}"#,
        )
        .unwrap();
        assert!(is_plain(&p).unwrap());
        let c = read(&p).unwrap();
        assert_eq!(c.len(), 2);
        let a = &c[0];
        assert_eq!(
            (a.code.as_str(), a.iso2.as_deref(), a.wikidata.as_deref()),
            ("ZZA", Some("za"), Some("Q1"))
        );
        assert_eq!(
            a.names,
            [
                (None, "A".to_string()),
                (Some("de"), "A-de".to_string()),
                (Some("zh-Hant"), "甲".to_string())
            ]
        );
        assert_eq!(
            (a.label, a.population, a.polygon.rings.len()),
            (Some((5_000_000, 2_500_000)), Some(1000), 2)
        );
        let b = &c[1];
        assert_eq!((b.iso2.as_deref(), b.wikidata.as_deref()), (None, None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_admin1_unit_with_its_country_ids_and_class() {
        let dir = std::env::temp_dir().join(format!("forge-ne1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ne1.geojson");
        std::fs::write(
            &p,
            r#"{"type":"FeatureCollection","features":[
              {"type":"Feature","properties":{"adm1_code":"ZZA-2","adm0_a3":"ZZA","name":"North","name_fr":"Nord",
                "wikidataid":"Q9","gn_id":123.0,"longitude":0.5,"latitude":0.25,"FCLASS_NP":"Unrecognized"},
               "geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}},
              {"type":"Feature","properties":{"adm1_code":"ZZA-1","adm0_a3":"ZZA","name":"South","gn_id":-1.0,"FCLASS_NP":null},
               "geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,-1],[0,0]]]}}]}"#,
        )
        .unwrap();
        let u = read_admin1(&p).unwrap();
        assert_eq!(
            u.iter().map(|u| u.code.as_str()).collect::<Vec<_>>(),
            ["ZZA-1", "ZZA-2"]
        );
        let n = &u[1];
        assert_eq!(
            (
                n.adm0.as_str(),
                n.wikidata.as_deref(),
                n.geonames,
                n.label,
                n.unrecognized
            ),
            (
                "ZZA",
                Some("Q9"),
                Some(123),
                Some((5_000_000, 2_500_000)),
                true
            )
        );
        assert_eq!(
            n.names,
            [
                (None, "North".to_string()),
                (Some("fr"), "Nord".to_string())
            ]
        );
        assert_eq!(
            (u[0].geonames, u[0].label, u[0].unrecognized),
            (None, None, false)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
