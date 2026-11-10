//! Timezone places: from timezone-boundary-builder (D-093, main build), or from tzdb's
//! `zone.tab` over the country's admin-0 outline (D-138, the lite build: a country with one
//! zone gets it over its own outline).

use super::Place;
use super::PlaceName;
use crate::geom::Polygon;
use crate::lock::Lock;
use std::fs;
use std::path::Path;
use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;

/// The timezone zones of a pack: their places and rings, in precedence order.
#[derive(Debug, Default)]
pub(crate) struct Zones {
    pub(crate) places: Vec<Place>,
    pub(crate) rings: Vec<Vec<Vec<[i32; 2]>>>,
}

#[derive(serde::Deserialize)]
pub(super) struct ZoneCollection {
    pub(super) features: Vec<ZoneFeature>,
}

#[derive(serde::Deserialize)]
pub(super) struct ZoneFeature {
    pub(super) properties: ZoneProps,
    pub(super) geometry: ZoneGeometry,
}

#[derive(serde::Deserialize)]
pub(super) struct ZoneProps {
    pub(super) tzid: String,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", content = "coordinates")]
pub(super) enum ZoneGeometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

/// The zones of timezone `source` (none without one). A gzip file is a tzdb release: the country `cc`'s one zone
/// over the admin-0 outline in the build's boundary source ([`tzdb_zones`]). Anything else is a
/// timezone-boundary-builder archive: its zones whose box meets `coverage`, those named in
/// `--tz-first` first (they win an overlap, D-084: the recipe's perspective), then in the source's
/// order.
pub(super) fn zone_places(
    root: &Path,
    lock: &Lock,
    source: Option<&str>,
    coverage: (i32, i32, i32, i32),
    a: &super::Args,
) -> Result<Zones, String> {
    use std::io::Read as _;
    let Some(source) = source else {
        return Ok(Zones::default());
    };
    let (first, cc, admin) = (&a.tz_first, a.cc.as_str(), a.admin.as_str());
    let zip_path = lock.path(root, source)?;
    let mut magic = [0_u8; 2];
    fs::File::open(&zip_path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|e| format!("{}: {e}", zip_path.display()))?;
    if magic == [0x1f, 0x8b] {
        return tzdb_zones(&zip_path, cc, &lock.path(root, admin)?);
    }
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
    zones.sort_by_key(|(id, _)| !first.contains(id));
    to_places(zones)
}

/// The places and rings of `zones` (ID, rings on the 1e-7° grid), in that order.
fn to_places(zones: Vec<(String, Vec<Vec<[i32; 2]>>)>) -> Result<Zones, String> {
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
            flags: 0,
            point,
            names: vec![PlaceName {
                text: tzid,
                lang: None,
                kind: Some(NameType::Official),
            }],
            population: None,
            rank: 0,
            parent: None,
        });
        out.rings.push(rings);
    }
    Ok(out)
}

/// The timezone each record of the read `sources` states, by record key (D-138).
pub(super) fn stated(
    build: &Path,
    sources: &[String],
) -> Result<std::collections::BTreeMap<String, String>, String> {
    use arrow_array::{Array, StringArray};
    let mut out = std::collections::BTreeMap::new();
    for s in sources {
        let file = build.join("read").join(s).join("records.parquet");
        crate::record::for_each_batch(&file, |b| {
            let ids = crate::record::column::<StringArray>(b, "id")?;
            let tz = crate::record::column::<StringArray>(b, "timezone")?;
            for i in (0..b.num_rows()).filter(|&i| tz.is_valid(i)) {
                out.insert(ids.value(i).to_string(), tz.value(i).to_string());
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// The zone of country `cc` from the tzdb release at `tarball` (D-138): its one row in
/// `zone.tab`, whose ID is the country's own (as timezone-boundary-builder names it), over the
/// admin-0 outline of the boundary archive `admin`. A country with several rows, or none, is
/// refused: it needs a table of admin units to zones, which no country has yet.
fn tzdb_zones(tarball: &Path, cc: &str, admin: &Path) -> Result<Zones, String> {
    let text = tar_file(tarball, "zone.tab")?;
    let cc = cc.to_ascii_uppercase();
    let rows = country_zones(&text, &cc);
    let [zone] = rows.as_slice() else {
        return Err(format!(
            "{cc}: tzdb's zone.tab lists {} zones ({}); the lite build takes a country with one, \
             and a table of admin units to zones for the others is not built yet",
            rows.len(),
            rows.join(", ")
        ));
    };
    let outline = crate::read::boundaries::read(admin, 0)?;
    let rings: Vec<Vec<[i32; 2]>> = outline
        .iter()
        .flat_map(|u| &u.polygon.rings)
        .map(|r| r.iter().map(|&(x, y)| [x, y]).collect())
        .collect();
    if rings.is_empty() {
        return Err(format!("{cc}: the boundary source has no admin-0 outline"));
    }
    to_places(vec![((*zone).to_string(), rings)])
}

/// The zone IDs `zone.tab` lists for country `cc` (upper case), in file order.
fn country_zones<'a>(zone_tab: &'a str, cc: &str) -> Vec<&'a str> {
    zone_tab
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split('\t');
            (f.next() == Some(cc)).then(|| f.nth(1)).flatten()
        })
        .collect()
}

/// The text of file `name` in the gzipped ustar archive at `path`.
pub(super) fn tar_file(path: &Path, name: &str) -> Result<String, String> {
    use std::io::Read as _;
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(file)
        .read_to_end(&mut tar)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut at = 0;
    while let Some(header) = tar.get(at..at + 512) {
        let field = |r: core::ops::Range<usize>| {
            let b = header.get(r).unwrap_or_default();
            let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf8_lossy(b.get(..end).unwrap_or_default())
                .trim()
                .to_string()
        };
        let entry = field(0..100);
        if entry.is_empty() {
            break;
        }
        let size = usize::from_str_radix(&field(124..136), 8)
            .map_err(|_| format!("{}: a tar header with a bad size", path.display()))?;
        let body = at + 512;
        if entry.trim_start_matches("./") == name {
            let bytes = tar
                .get(body..body + size)
                .ok_or_else(|| format!("{}: {name} is cut short", path.display()))?;
            return String::from_utf8(bytes.to_vec()).map_err(|e| format!("{name}: {e}"));
        }
        at = body + size.div_ceil(512) * 512;
    }
    Err(format!("{}: no {name}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::country_zones;

    #[test]
    fn the_zones_of_a_country_are_its_rows_of_zone_tab() {
        let tab = "# comment NP\nNP\t+2743+08519\tAsia/Kathmandu\nIN\t+2232+08822\tAsia/Kolkata\n\
                   US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\n\
                   US\t+415100-0873900\tAmerica/Chicago\tCentral (most areas)\n";
        assert_eq!(country_zones(tab, "NP"), ["Asia/Kathmandu"]);
        assert_eq!(
            country_zones(tab, "US"),
            ["America/New_York", "America/Chicago"]
        );
        assert_eq!(country_zones(tab, "NPL"), Vec::<&str>::new());
        assert_eq!(country_zones(tab, "XX"), Vec::<&str>::new());
    }
}
