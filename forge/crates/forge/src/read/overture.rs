//! Overture Maps divisions (D-155): the land areas of countries, dependencies and regions from
//! `division_area`, and their Wikidata items and OSM relations from `division`, read from the
//! pinned `GeoParquet` part files of the `overture-divisions` tileset.
//!
//! Each file is streamed a batch at a time. A row filter on `subtype` and `class` runs before
//! the other columns are decoded, so the geometry of the 98% of rows the baseline does not use
//! (counties, localities, maritime areas) is never decoded. Geometry is WKB (a 2D polygon or
//! multipolygon), put on the 1e-7° grid; a multipolygon's parts become rings of one even-odd
//! polygon, as in [`super::natural_earth`].

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use arrow_array::{
    Array, BinaryArray, BooleanArray, Int32Array, ListArray, MapArray, RecordBatch, StringArray,
    StructArray,
};
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::{ArrowPredicateFn, ParquetRecordBatchReaderBuilder, RowFilter};
use rayon::prelude::*;

use crate::geom::{Bbox, Polygon};
use crate::record::e7;

/// What a division is, of the subtypes the baseline reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Subtype {
    Country,
    Dependency,
    Region,
}

impl Subtype {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "country" => Some(Self::Country),
            "dependency" => Some(Self::Dependency),
            "region" => Some(Self::Region),
            _ => None,
        }
    }
}

/// One land area of a country, dependency or region.
#[derive(Debug)]
pub(crate) struct Area {
    /// The division's GERS ID (`division_id`), unique among land areas.
    pub(crate) division: String,
    pub(crate) subtype: Subtype,
    /// ISO 3166-1 alpha-2 (or Overture's synthetic code) of the country or dependency it is in.
    pub(crate) country: String,
    /// Its place in its own country's hierarchy (1 for a region under a country).
    pub(crate) admin_level: Option<i32>,
    /// ISO 3166-2 code, when it has one.
    pub(crate) region: Option<String>,
    /// The primary name first (untagged), then English when it differs.
    pub(crate) names: Vec<(Option<&'static str>, String)>,
    pub(crate) polygon: Polygon,
}

/// The cross-references of a division.
#[derive(Debug, Default)]
pub(crate) struct Division {
    /// `Q…`, when set.
    pub(crate) wikidata: Option<String>,
    /// The OSM relation it was made from (`r1943188@152` → 1943188).
    pub(crate) osm_relation: Option<u64>,
}

/// The land areas of every country, dependency and region in `files`, sorted by division ID.
///
/// # Errors
/// An unreadable file, a column of an unexpected type, bad WKB, or a division twice.
pub(crate) fn read_areas(files: &[PathBuf]) -> Result<Vec<Area>, String> {
    let parts: Vec<Vec<Area>> = files
        .par_iter()
        .map(|f| read_area_file(f).map_err(|e| format!("{}: {e}", f.display())))
        .collect::<Result<_, _>>()?;
    let mut out: Vec<Area> = parts.into_iter().flatten().collect();
    out.sort_by(|a, b| a.division.cmp(&b.division));
    if let Some(id) = out.windows(2).find_map(|w| match w {
        [a, b] if a.division == b.division => Some(&a.division),
        _ => None,
    }) {
        return Err(format!("division {id} has two land areas"));
    }
    Ok(out)
}

/// The cross-references of every country, dependency and region in `files`, by division ID.
///
/// # Errors
/// An unreadable file or a column of an unexpected type.
pub(crate) fn read_divisions(files: &[PathBuf]) -> Result<HashMap<String, Division>, String> {
    let mut out = HashMap::new();
    for f in files {
        for_each(f, &["id", "subtype", "wikidata", "sources"], false, |b| {
            let ids = strings(b, "id")?;
            let wikidata = strings(b, "wikidata")?;
            let sources = b
                .column_by_name("sources")
                .and_then(|c| c.as_any().downcast_ref::<ListArray>())
                .ok_or("no list column sources")?;
            for i in 0..b.num_rows() {
                let osm_relation = if sources.is_valid(i) {
                    relation(&sources.value(i))?
                } else {
                    None
                };
                out.insert(
                    ids.value(i).to_owned(),
                    Division {
                        wikidata: wikidata
                            .is_valid(i)
                            .then(|| wikidata.value(i))
                            .filter(|q| q.starts_with('Q'))
                            .map(str::to_owned),
                        osm_relation,
                    },
                );
            }
            Ok(())
        })
        .map_err(|e| format!("{}: {e}", f.display()))?;
    }
    Ok(out)
}

/// A unit of the world's land partition: a region, or land of a country or dependency that no
/// region of it covers (all of it when it has none).
#[derive(Debug)]
pub(crate) struct LandUnit {
    /// The index of its area in the input: the region, or the country or dependency.
    pub(crate) area: usize,
    pub(crate) rings: Vec<Vec<(i32, i32)>>,
}

/// A country's land outside every region narrower than this on average, in 1e-7° (about
/// 100 m, the tolerance the world's land is thinned to, D-155), is two drawings of one coast,
/// not land: Overture clips a country's land and its regions' to slightly different coasts
/// (99,743 such parts at least 1 m wide in 2026-10-21.0, Spain alone 7,160).
pub(crate) const MIN_LAND_WIDTH: i128 = 9_000;

/// The world's land as a partition of units, deterministic in country-code order:
///
/// 0. **Zones:** where two countries' land overlaps (claims: the Southern Patagonian Ice
///    Field in Argentina's and Chile's, Machias Seal Island in Canada's and the United
///    States'), the overlap at least 1 m wide is the later code's zone.
/// 1. **Regions,** each without the zones of its own country: the earlier code keeps the claim.
///    A region no zone meets keeps its own vertices, with no overlay.
/// 2. **Land no region covers:** each country's or dependency's land without the final regions
///    and its own zones, in parts at least [`MIN_LAND_WIDTH`] wide: lakes no region holds, a
///    claim no region holds (Western Sahara in Morocco's land), all of a country with no
///    region. The regions are disjoint by then, so their union is one even–odd shape.
///
/// Which country owns what is decided later, by the point of view; this only makes the units
/// disjoint, so they share arcs.
///
/// # Errors
/// A region whose `country` names no country or dependency, or a code held twice.
pub(crate) fn land_units(areas: &[Area]) -> Result<Vec<LandUnit>, String> {
    use crate::topo::pov::{MIN_WIDTH, common, minus};
    let mut tops: Vec<(&str, usize)> = Vec::new();
    let mut regions: Vec<(&str, usize)> = Vec::new();
    for (i, a) in areas.iter().enumerate() {
        match a.subtype {
            Subtype::Region => regions.push((&a.country, i)),
            Subtype::Country | Subtype::Dependency => tops.push((&a.country, i)),
        }
    }
    tops.sort_unstable();
    let division = |i: usize| areas.get(i).map_or("", |a| a.division.as_str());
    regions.sort_unstable_by(|x, y| x.0.cmp(y.0).then(division(x.1).cmp(division(y.1))));
    if let Some(cc) = tops.windows(2).find_map(|w| match w {
        [x, y] if x.0 == y.0 => Some(x.0),
        _ => None,
    }) {
        return Err(format!("{cc}: two countries or dependencies"));
    }
    if let Some((cc, _)) = regions
        .iter()
        .find(|(cc, _)| tops.binary_search_by(|t| t.0.cmp(cc)).is_err())
    {
        return Err(format!(
            "regions of {cc}, which is no country or dependency"
        ));
    }
    let boxes: Vec<Boxes> = areas.iter().map(|a| Boxes::of(&a.polygon.rings)).collect();
    let no_box = Boxes::default();
    let bx = |i: usize| boxes.get(i).unwrap_or(&no_box);
    let rings = |i: usize| areas.get(i).map_or(&[][..], |a| a.polygon.rings.as_slice());
    // 0. Zones: (the later country's code, the overlap).
    let pairs: Vec<(usize, usize)> = tops
        .iter()
        .enumerate()
        .flat_map(|(x, &(_, i))| {
            tops.iter()
                .skip(x + 1)
                .filter(move |&&(_, j)| bx(i).meets(bx(j)))
                .map(move |&(_, j)| (i, j))
        })
        .collect();
    let zones: Vec<Zone<'_>> = pairs
        .par_iter()
        .filter_map(|&(i, j)| {
            let near_i = bx(i).near(rings(i), bx(j));
            let near_j = bx(j).near(rings(j), bx(i));
            let z = common(&near_i, &near_j, MIN_WIDTH);
            let cc = areas.get(j).map(|a| a.country.as_str())?;
            (!z.is_empty()).then(|| (cc, Boxes::of(&z), z))
        })
        .collect();
    let zones_of = |cc: &str, b: &Boxes| -> Vec<&[Vec<(i32, i32)>]> {
        zones
            .iter()
            .filter(|(z, zb, _)| *z == cc && zb.meets(b))
            .map(|(_, _, r)| r.as_slice())
            .collect()
    };
    // 1. Regions without their country's zones.
    let cut: Vec<Parts> = regions
        .par_iter()
        .map(|&(cc, i)| {
            let z = zones_of(cc, bx(i));
            if z.is_empty() {
                return vec![rings(i).to_vec()];
            }
            minus(rings(i), &z, MIN_WIDTH).unwrap_or_else(|| vec![rings(i).to_vec()])
        })
        .collect();
    let mut out: Vec<LandUnit> = regions
        .iter()
        .zip(cut)
        .flat_map(|(&(_, i), parts)| {
            parts
                .into_iter()
                .map(move |r| LandUnit { area: i, rings: r })
        })
        .collect();
    disjoint_within_countries(areas, &mut out);
    // 2. Each country's land without the regions near it and its own zones.
    let unit_boxes: Vec<Boxes> = out.iter().map(|u| Boxes::of(&u.rings)).collect();
    let rest: Vec<Parts> = tops
        .par_iter()
        .map(|&(cc, top)| {
            let mut clips: Vec<&[Vec<(i32, i32)>]> = out
                .iter()
                .zip(&unit_boxes)
                .filter(|(_, b)| b.meets(bx(top)))
                .map(|(u, _)| u.rings.as_slice())
                .collect();
            clips.extend(zones_of(cc, bx(top)));
            if clips.is_empty() {
                return vec![rings(top).to_vec()];
            }
            minus(rings(top), &clips, MIN_LAND_WIDTH).unwrap_or_else(|| vec![rings(top).to_vec()])
        })
        .collect();
    for (&(_, top), parts) in tops.iter().zip(rest) {
        out.extend(parts.into_iter().map(|rings| LandUnit { area: top, rings }));
    }
    Ok(out)
}

/// Makes the regions of each country disjoint (D-155): Overture can hold one area twice in a
/// country, as Morocco's Dakhla-Oued Ed-Dahab (MA-12) and a region "Western Sahara" without a
/// code over it. A country is checked with one overlay (union area against summed areas); only
/// a country that overlaps is resolved pairwise: a region with an ISO 3166-2 code keeps the
/// overlap ahead of one without, then the lower division ID.
fn disjoint_within_countries(areas: &[Area], units: &mut Vec<LandUnit>) {
    use crate::topo::pov::{MIN_WIDTH, common, minus, overlapping};
    let country = |u: &LandUnit| areas.get(u.area).map_or("", |a| a.country.as_str());
    let rank = |u: &LandUnit| {
        areas
            .get(u.area)
            .map_or((true, ""), |a| (a.region.is_none(), a.division.as_str()))
    };
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (k, u) in units.iter().enumerate() {
        match groups.last_mut() {
            Some(g)
                if g.first()
                    .is_some_and(|&f| units.get(f).map(country) == Some(country(u))) =>
            {
                g.push(k);
            }
            _ => groups.push(vec![k]),
        }
    }
    let fixes: Vec<Vec<(usize, Parts)>> = groups
        .par_iter()
        .map(|g| {
            let sets: Vec<&[Vec<(i32, i32)>]> = g
                .iter()
                .filter_map(|&k| units.get(k))
                .map(|u| u.rings.as_slice())
                .collect();
            if g.len() < 2 || !overlapping(&sets) {
                return Vec::new();
            }
            let mut order = g.clone();
            order.sort_by_key(|&k| units.get(k).map(rank));
            let boxes: Vec<Boxes> = order
                .iter()
                .map(|&k| Boxes::of(units.get(k).map_or(&[][..], |u| u.rings.as_slice())))
                .collect();
            let mut fixed = Vec::new();
            for (x, &k) in order.iter().enumerate() {
                let Some(u) = units.get(k) else { continue };
                let clips: Vec<&[Vec<(i32, i32)>]> = order
                    .iter()
                    .zip(&boxes)
                    .take(x)
                    .filter(|&(_, b)| boxes.get(x).is_some_and(|own| b.meets(own)))
                    .filter_map(|(&j, _)| units.get(j))
                    .map(|v| v.rings.as_slice())
                    .filter(|r| !common(&u.rings, r, MIN_WIDTH).is_empty())
                    .collect();
                if let Some(parts) = minus(&u.rings, &clips, MIN_WIDTH) {
                    fixed.push((k, parts));
                }
            }
            fixed
        })
        .collect();
    let mut drop = vec![false; units.len()];
    let mut extra = Vec::new();
    for (k, parts) in fixes.into_iter().flatten() {
        let area = units.get(k).map_or(0, |u| u.area);
        if let Some(d) = drop.get_mut(k) {
            *d = true;
        }
        extra.extend(parts.into_iter().map(|rings| LandUnit { area, rings }));
    }
    let mut k = 0;
    units.retain(|_| {
        k += 1;
        !drop.get(k - 1).copied().unwrap_or(false)
    });
    units.extend(extra);
}

/// Polygons as parts, each a list of rings.
type Parts = Vec<Vec<Vec<(i32, i32)>>>;

/// A zone: the later country's code, its boxes and its rings.
type Zone<'a> = (&'a str, Boxes, Vec<Vec<(i32, i32)>>);

/// The boxes of a set of rings: the whole and each ring's. Two sets meet where their wholes
/// meet and, when either spans the antimeridian (wider than 180°), where two rings' boxes do.
#[derive(Debug, Default)]
struct Boxes {
    whole: Option<Bbox>,
    rings: Vec<Option<Bbox>>,
}

impl Boxes {
    fn of(rings: &[Vec<(i32, i32)>]) -> Self {
        Self {
            whole: bbox_of(rings),
            rings: rings
                .iter()
                .map(|r| bbox_of(std::slice::from_ref(r)))
                .collect(),
        }
    }

    fn spans(&self) -> bool {
        self.whole
            .is_some_and(|b| i64::from(b.2) - i64::from(b.0) > 1_800_000_000)
    }

    fn meets(&self, other: &Self) -> bool {
        if !meets(self.whole, other.whole) {
            return false;
        }
        if !self.spans() && !other.spans() {
            return true;
        }
        self.rings
            .iter()
            .any(|&a| meets(a, other.whole) && other.rings.iter().any(|&b| meets(a, b)))
    }

    /// The rings of `rings` (whose boxes these are) that meet `other`.
    fn near(&self, rings: &[Vec<(i32, i32)>], other: &Self) -> Vec<Vec<(i32, i32)>> {
        rings
            .iter()
            .zip(&self.rings)
            .filter(|&(_, &b)| {
                meets(b, other.whole)
                    && (!other.spans() || other.rings.iter().any(|&o| meets(b, o)))
            })
            .map(|(r, _)| r.clone())
            .collect()
    }
}

/// Whether two boxes meet.
fn meets(a: Option<Bbox>, b: Option<Bbox>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3,
        _ => false,
    }
}

/// The bounding box of rings.
fn bbox_of(rings: &[Vec<(i32, i32)>]) -> Option<Bbox> {
    let mut it = rings.iter().flatten();
    let &(x, y) = it.next()?;
    Some(it.fold((x, y, x, y), |(a, b, c, d), &(x, y)| {
        (a.min(x), b.min(y), c.max(x), d.max(y))
    }))
}

/// The OSM relation among a row's sources (`dataset` `OpenStreetMap`, `record_id` `r<id>@<v>`).
fn relation(sources: &dyn Array) -> Result<Option<u64>, String> {
    let s = sources
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or("sources is not a list of structs")?;
    let col = |n: &str| {
        s.column_by_name(n)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| format!("sources has no string field {n}"))
    };
    let (dataset, record) = (col("dataset")?, col("record_id")?);
    Ok((0..s.len()).find_map(|i| {
        (dataset.is_valid(i) && dataset.value(i) == "OpenStreetMap" && record.is_valid(i))
            .then(|| record.value(i))
            .and_then(|r| r.strip_prefix('r'))
            .and_then(|r| r.split('@').next())
            .and_then(|r| r.parse().ok())
    }))
}

fn read_area_file(path: &Path) -> Result<Vec<Area>, String> {
    let mut out = Vec::new();
    let cols = [
        "division_id",
        "subtype",
        "class",
        "country",
        "admin_level",
        "region",
        "names",
        "geometry",
    ];
    for_each(path, &cols, true, |b| {
        let division = strings(b, "division_id")?;
        let subtype = strings(b, "subtype")?;
        let country = strings(b, "country")?;
        let region = strings(b, "region")?;
        let level = b
            .column_by_name("admin_level")
            .and_then(|c| c.as_any().downcast_ref::<Int32Array>())
            .ok_or("no int column admin_level")?;
        let geometry = b
            .column_by_name("geometry")
            .and_then(|c| c.as_any().downcast_ref::<BinaryArray>())
            .ok_or("no binary column geometry")?;
        let names = b
            .column_by_name("names")
            .and_then(|c| c.as_any().downcast_ref::<StructArray>())
            .ok_or("no struct column names")?;
        for i in 0..b.num_rows() {
            let id = division.value(i);
            let sub = Subtype::parse(subtype.value(i))
                .ok_or_else(|| format!("{id}: subtype {}", subtype.value(i)))?;
            if !geometry.is_valid(i) || !country.is_valid(i) {
                return Err(format!("{id}: no geometry or country"));
            }
            out.push(Area {
                division: id.to_owned(),
                subtype: sub,
                country: country.value(i).to_owned(),
                admin_level: level.is_valid(i).then(|| level.value(i)),
                region: region.is_valid(i).then(|| region.value(i).to_owned()),
                names: area_names(names, i)?,
                polygon: Polygon {
                    rings: wkb(geometry.value(i)).map_err(|e| format!("{id}: {e}"))?,
                },
            });
        }
        Ok(())
    })?;
    Ok(out)
}

/// The primary name, then the English one when it differs.
fn area_names(
    names: &StructArray,
    i: usize,
) -> Result<Vec<(Option<&'static str>, String)>, String> {
    let mut out = Vec::new();
    if !names.is_valid(i) {
        return Ok(out);
    }
    let primary = names
        .column_by_name("primary")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .ok_or("names has no string field primary")?;
    if primary.is_valid(i) && !primary.value(i).trim().is_empty() {
        out.push((None, primary.value(i).trim().to_owned()));
    }
    let common = names
        .column_by_name("common")
        .and_then(|c| c.as_any().downcast_ref::<MapArray>())
        .ok_or("names has no map field common")?;
    if common.is_valid(i) {
        let entries = common.value(i);
        let keys = entries
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or("names.common keys are not strings")?;
        let values = entries
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or("names.common values are not strings")?;
        if let Some(en) = (0..keys.len()).find(|&k| keys.value(k) == "en" && values.is_valid(k)) {
            let en = values.value(en).trim();
            if !en.is_empty() && out.iter().all(|(_, n)| n != en) {
                out.push((Some("en"), en.to_owned()));
            }
        }
    }
    Ok(out)
}

/// Streams `cols` of a Parquet file a batch at a time. With `filter`, only rows of a country,
/// dependency or region whose `class` is `land` are decoded.
fn for_each(
    path: &Path,
    cols: &[&str],
    filter: bool,
    mut f: impl FnMut(&RecordBatch) -> Result<(), String>,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| e.to_string())?;
    let roots = |names: &[&str]| -> Result<ProjectionMask, String> {
        let fields = builder.schema().fields();
        let idx: Vec<usize> = names
            .iter()
            .map(|n| {
                fields
                    .iter()
                    .position(|f| f.name() == n)
                    .ok_or_else(|| format!("no column {n}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(ProjectionMask::roots(builder.parquet_schema(), idx))
    };
    let mask = roots(cols)?;
    let keep_mask = roots(if filter {
        &["subtype", "class"]
    } else {
        &["subtype"]
    })?;
    let keep = ArrowPredicateFn::new(keep_mask, move |b: RecordBatch| {
        let col = |n: &str| {
            b.column_by_name(n)
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                .cloned()
                .ok_or_else(|| {
                    arrow_schema::ArrowError::SchemaError(format!("no string column {n}"))
                })
        };
        let subtype = col("subtype")?;
        let class = if filter { Some(col("class")?) } else { None };
        Ok((0..b.num_rows())
            .map(|i| {
                Some(
                    subtype.is_valid(i)
                        && Subtype::parse(subtype.value(i)).is_some()
                        && class
                            .as_ref()
                            .is_none_or(|c| c.is_valid(i) && c.value(i) == "land"),
                )
            })
            .collect::<BooleanArray>())
    });
    builder = builder
        .with_projection(mask)
        .with_row_filter(RowFilter::new(vec![Box::new(keep)]));
    for batch in builder.build().map_err(|e| e.to_string())? {
        f(&batch.map_err(|e| e.to_string())?)?;
    }
    Ok(())
}

fn strings<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a StringArray, String> {
    b.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| format!("no string column {name}"))
}

/// A reader of WKB that checks every count against the bytes left.
struct Wkb<'a> {
    b: &'a [u8],
    at: usize,
    little: bool,
}

impl Wkb<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let s = self.b.get(self.at..self.at + N).ok_or("WKB ends early")?;
        self.at += N;
        let mut a = [0; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let a = self.take::<4>()?;
        Ok(if self.little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }

    fn f64(&mut self) -> Result<f64, String> {
        let a = self.take::<8>()?;
        Ok(if self.little {
            f64::from_le_bytes(a)
        } else {
            f64::from_be_bytes(a)
        })
    }

    /// A count of items at least `min` bytes each: no more than the bytes left allow.
    fn count(&mut self, min: usize) -> Result<usize, String> {
        let n = usize::try_from(self.u32()?).map_err(|e| e.to_string())?;
        if n > (self.b.len() - self.at) / min {
            return Err(format!("WKB count {n} is more than its bytes hold"));
        }
        Ok(n)
    }

    /// A geometry header: its byte order, then its type.
    fn header(&mut self) -> Result<u32, String> {
        self.little = match self.take::<1>()? {
            [1] => true,
            [0] => false,
            [o] => return Err(format!("WKB byte order {o}")),
        };
        self.u32()
    }

    /// A whole 2D Polygon geometry: its header, then its rings.
    fn polygon_whole(&mut self, out: &mut Vec<Vec<(i32, i32)>>) -> Result<(), String> {
        match self.header()? {
            3 => self.polygon(out),
            t => Err(format!("WKB type {t} inside a multipolygon")),
        }
    }

    fn polygon(&mut self, out: &mut Vec<Vec<(i32, i32)>>) -> Result<(), String> {
        for _ in 0..self.count(4)? {
            let n = self.count(16)?;
            let mut ring = Vec::with_capacity(n);
            for _ in 0..n {
                let (lon, lat) = (self.f64()?, self.f64()?);
                ring.push(
                    e7(lon)
                        .zip(e7(lat))
                        .ok_or_else(|| format!("WKB point ({lon}, {lat}) is off the globe"))?,
                );
            }
            out.push(ring);
        }
        Ok(())
    }
}

/// The rings of a WKB polygon (type 3) or multipolygon (type 6), 2D, on the 1e-7° grid.
fn wkb(b: &[u8]) -> Result<Vec<Vec<(i32, i32)>>, String> {
    let mut r = Wkb {
        b,
        at: 0,
        little: true,
    };
    let mut rings = Vec::new();
    match r.header()? {
        3 => r.polygon(&mut rings)?,
        6 => {
            // Each part is at least a header and a ring count: 9 bytes.
            for _ in 0..r.count(9)? {
                r.polygon_whole(&mut rings)?;
            }
        }
        t => return Err(format!("WKB type {t}: not a 2D polygon or multipolygon")),
    }
    if r.at != b.len() {
        return Err(format!("{} bytes after the WKB geometry", b.len() - r.at));
    }
    Ok(rings)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::wkb;

    fn polygon(little: bool, rings: &[&[(f64, f64)]]) -> Vec<u8> {
        let u = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let f = |v: f64| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut b = vec![u8::from(little)];
        b.extend(u(3));
        b.extend(u(u32::try_from(rings.len()).unwrap()));
        for r in rings {
            b.extend(u(u32::try_from(r.len()).unwrap()));
            for &(x, y) in *r {
                b.extend(f(x));
                b.extend(f(y));
            }
        }
        b
    }

    #[test]
    fn polygons_and_multipolygons_in_either_byte_order() {
        let ring: &[(f64, f64)] = &[(85.0, 27.0), (86.0, 27.0), (86.0, 28.0), (85.0, 27.0)];
        for little in [true, false] {
            let p = polygon(little, &[ring]);
            let got = wkb(&p).unwrap();
            assert_eq!(got[0][1], (860_000_000, 270_000_000));
            let mut m = vec![u8::from(little)];
            let u = |v: u32| {
                if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                }
            };
            m.extend(u(6));
            m.extend(u(2));
            m.extend(&p);
            m.extend(&p);
            assert_eq!(wkb(&m).unwrap().len(), 2);
        }
    }

    #[test]
    fn lying_counts_short_bytes_and_other_types_fail() {
        let ring: &[(f64, f64)] = &[(85.0, 27.0), (86.0, 27.0), (86.0, 28.0)];
        let p = polygon(true, &[ring]);
        // A ring count far beyond the bytes.
        let mut lie = p.clone();
        lie[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(wkb(&lie).is_err());
        assert!(wkb(&p[..p.len() - 1]).is_err());
        let mut extra = p.clone();
        extra.push(0);
        assert!(wkb(&extra).is_err());
        let mut point = p;
        point[1..5].copy_from_slice(&1u32.to_le_bytes());
        assert!(wkb(&point).is_err());
        assert!(wkb(&[]).is_err());
        // Off the globe.
        assert!(wkb(&polygon(true, &[&[(400.0, 0.0), (0.0, 0.0), (0.0, 1.0)]])).is_err());
    }
}

/// Differential gate against `DuckDB` 2.0.0's reading of the same files (2026-11-10, the research
/// note §3.2): run with `cargo test --release -- --ignored overture` once
/// `forge fetch overture-divisions` has stored the tiles.
#[cfg(test)]
mod real {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::print_stderr,
        reason = "test code: a measurement that reports what it found"
    )]

    use std::collections::HashMap;
    use std::path::Path;

    use super::{Subtype, read_areas, read_divisions};
    use crate::lock::Lock;
    use crate::tiles;

    /// The world's land as one partition, thinned at level 3 (100 m) as shared arcs (D-155):
    /// it must be a partition (no overlap) and the guard must leave nothing. Run with
    /// `cargo test --profile fast -- --ignored --nocapture overture_land` under a memory cap.
    #[test]
    #[ignore = "needs the overture-divisions tiles (5 GB)"]
    fn overture_land_thins_as_one_partition() {
        let t = std::time::Instant::now();
        let areas = read_areas(&files("area-")).unwrap();
        let units = super::land_units(&areas).unwrap();
        let fills = units
            .iter()
            .filter(|u| areas[u.area].subtype != Subtype::Region)
            .count();
        eprintln!(
            "{} land units ({fills} country land outside regions) in {:.1?}",
            units.len(),
            t.elapsed()
        );
        let mut sizes: Vec<(f64, String)> = units
            .iter()
            .filter(|u| areas[u.area].subtype != Subtype::Region)
            .filter(|u| {
                areas
                    .iter()
                    .any(|a| a.subtype == Subtype::Region && a.country == areas[u.area].country)
            })
            .map(|u| {
                let m2: f64 = u
                    .rings
                    .iter()
                    .map(|r| crate::topo::measure::ring_area_m2(r).abs())
                    .sum();
                (m2, areas[u.area].country.clone())
            })
            .collect();
        sizes.sort_by(|a, b| b.0.total_cmp(&a.0));
        let total: f64 = sizes.iter().map(|s| s.0).sum();
        let over = |t: f64| sizes.iter().filter(|s| s.0 > t).count();
        eprintln!(
            "remainders in countries with regions: {} total {:.0} km2; >1 km2 {}, >10 km2 {}, >100 km2 {}; largest {:?}",
            sizes.len(),
            total / 1e6,
            over(1e6),
            over(1e7),
            over(1e8),
            sizes
                .iter()
                .take(25)
                .map(|(a, c)| format!("{c} {:.0}", a / 1e6))
                .collect::<Vec<_>>()
        );
        let mut by_cc: HashMap<&str, usize> = HashMap::new();
        for s in &sizes {
            *by_cc.entry(s.1.as_str()).or_default() += 1;
        }
        let mut by_cc: Vec<_> = by_cc.into_iter().collect();
        by_cc.sort_by_key(|x| std::cmp::Reverse(x.1));
        eprintln!("pieces per country: {:?}", &by_cc[..by_cc.len().min(15)]);
        drop(areas);
        let rings: Vec<&[Vec<(i32, i32)>]> = units.iter().map(|u| u.rings.as_slice()).collect();
        let (out, thinned) = crate::topo::thin::partition(&rings, 3).unwrap();
        eprintln!("{thinned:?} in {:.1?}", t.elapsed());
        assert_eq!(out.len(), units.len());
        assert_eq!(thinned.guard.left, 0, "{thinned:?}");
        assert!(thinned.vertices.1 * 4 < thinned.vertices.0, "{thinned:?}");
    }

    fn files(prefix: &str) -> Vec<std::path::PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let lock = Lock::read(&root.join("data/sources.lock")).unwrap();
        let s = lock.get("overture-divisions").unwrap();
        tiles::manifest(&root, s)
            .unwrap()
            .iter()
            .filter(|t| t.name.starts_with(prefix))
            .map(|t| tiles::tile_path(&root, t).unwrap())
            .collect()
    }

    #[test]
    #[ignore = "needs the overture-divisions tiles (5 GB)"]
    fn overture_reads_as_duckdb_does() {
        let areas = read_areas(&files("area-")).unwrap();
        let count = |k: Subtype| areas.iter().filter(|a| a.subtype == k).count();
        assert_eq!(
            (
                count(Subtype::Country),
                count(Subtype::Dependency),
                count(Subtype::Region)
            ),
            (219, 53, 3_929)
        );
        let vertices: usize = areas
            .iter()
            .filter(|a| a.subtype == Subtype::Region)
            .map(|a| a.polygon.rings.iter().map(Vec::len).sum::<usize>())
            .sum();
        assert_eq!(vertices, 29_508_797);
        let np = areas
            .iter()
            .filter(|a| a.country == "NP" && a.subtype == Subtype::Region)
            .count();
        assert_eq!(np, 7);
        let divisions = read_divisions(&files("division-")).unwrap();
        assert_eq!(divisions.len(), 219 + 53 + 3_929);
        assert_eq!(
            divisions.values().filter(|d| d.wikidata.is_some()).count(),
            212 + 53 + 3_913
        );
        assert!(areas.iter().all(|a| divisions.contains_key(&a.division)));
        let osm = divisions
            .values()
            .filter(|d| d.osm_relation.is_some())
            .count();
        assert_eq!(osm, 4_195);
    }
}
