//! Timezone places from timezone-boundary-builder (D-093).

use super::Place;
use super::PlaceName;
use crate::geom::Polygon;
use crate::lock::Lock;
use std::fs;
use std::path::Path;
use teigeodata_core::PlaceKind;
use teigeodata_format::names::NameType;

/// Zones that win an overlap, before the source's order (D-084): the Nepal perspective.
pub(super) const TZ_FIRST: [&str; 1] = ["Asia/Kathmandu"];

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

/// The zones of timezone-boundary-builder whose box meets `coverage`, `TZ_FIRST` first, then
/// in the source's order.
pub(super) fn zone_places(
    root: &Path,
    lock: &Lock,
    source: &str,
    coverage: (i32, i32, i32, i32),
) -> Result<Zones, String> {
    use std::io::Read as _;
    let zip_path = lock.path(root, source)?;
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
