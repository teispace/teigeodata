//! The `buildings/<source>` stages: one point per building, the input of the settlement core
//! (D-091, [settlement-core](../../../../docs/02-research/notes/settlement-core.md)).
//!
//! ```text
//! forge buildings <osm|msbuildings|google> <source-id> <west,south,east,north>
//! ```
//!
//! - `osm`: ways tagged `building` (not `building=no`) of a PBF extract. A building's point is
//!   its first node: off its centroid by at most the building's size, well inside the core's
//!   25 m cells, and it needs one node location per building instead of every vertex (8.3 M
//!   lookups for Nepal, not 45 M). Two passes: the ways give the nodes to look up, then the
//!   nodes give their locations.
//! - `msbuildings`: Microsoft Global ML Building Footprints, gzipped `GeoJSON` lines per quadkey;
//!   the point is the vertex mean of the outer ring (without its closing vertex).
//! - `google`: Google Open Buildings v3 points, gzipped CSV without a header
//!   (`latitude,longitude,area_in_meters,confidence,full_plus_code`).
//!
//! Points outside the extent (decimal degrees) are dropped. Tiles are read in parallel and the
//! output is sorted by (lat, lon), so it is the same bytes on every run. Output:
//! `data/build/buildings/<source-id>/points.parquet` (`lon_e7`, `lat_e7`, Int32) and
//! `summary.md`.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use arrow_array::{ArrayRef, Int32Array};
use arrow_schema::{DataType, Field};
use flate2::read::MultiGzDecoder;
use osmpbf::{Element, ElementReader};
use rayon::prelude::*;
use serde::Deserialize;

use crate::fetch::store_path;
use crate::lock::Lock;
use crate::record::{e7, write_table};
use crate::tiles::{manifest, tile_path};

/// A building's point on the 1e-7° grid: (lon, lat).
pub(crate) type Pt = (i32, i32);

/// The extent points are kept in, on the grid: (west, south, east, north), edges inclusive.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Extent(
    pub(crate) i32,
    pub(crate) i32,
    pub(crate) i32,
    pub(crate) i32,
);

impl Extent {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let v: Vec<i32> = text
            .split(',')
            .map(|x| x.trim().parse::<f64>().ok().and_then(e7))
            .collect::<Option<_>>()
            .ok_or_else(|| format!("extent `{text}`: expected west,south,east,north in degrees"))?;
        match v.as_slice() {
            &[west, south, east, north]
                if west < east
                    && south < north
                    && south >= -900_000_000
                    && north <= 900_000_000 =>
            {
                Ok(Self(west, south, east, north))
            }
            _ => Err(format!(
                "extent `{text}`: expected west < east and south < north"
            )),
        }
    }

    fn holds(self, (x, y): Pt) -> bool {
        (self.0..=self.2).contains(&x) && (self.1..=self.3).contains(&y)
    }
}

/// `forge buildings <reader> <source-id> <extent>`.
pub(crate) fn run(root: &Path, reader: &str, id: &str, extent: &str) -> Result<String, String> {
    let t = Instant::now();
    let ext = Extent::parse(extent)?;
    let lock = Lock::read(&root.join("data/sources.lock"))?;
    let s = lock.get(id)?;
    let (mut pts, read) = match reader {
        "osm" => {
            let path = store_path(root, s);
            if !path.is_file() {
                return Err(format!("{id} is not fetched yet: forge fetch {id}"));
            }
            osm(&path, ext)?
        }
        "msbuildings" | "google" => {
            let tiles = manifest(root, s)?;
            let paths: Vec<_> = tiles.iter().filter_map(|t| tile_path(root, t)).collect();
            if let Some(p) = paths.iter().find(|p| !p.is_file()) {
                return Err(format!(
                    "{id}: {} is not fetched yet: forge fetch {id}",
                    p.display()
                ));
            }
            let parse = if reader == "google" {
                google_line
            } else {
                ms_line
            };
            let per_tile: Vec<(Vec<Pt>, u64)> = paths
                .par_iter()
                .map(|p| gz_lines(p, ext, parse))
                .collect::<Result<_, _>>()?;
            let read = per_tile.iter().map(|t| t.1).sum();
            (per_tile.into_iter().flat_map(|t| t.0).collect(), read)
        }
        r => {
            return Err(format!(
                "unknown buildings reader `{r}` (osm, msbuildings, google)"
            ));
        }
    };
    pts.par_sort_unstable_by_key(|&(x, y)| (y, x));
    let dir = root.join("data/build/buildings").join(id);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_table(
        &dir.join("points.parquet"),
        vec![
            Field::new("lon_e7", DataType::Int32, false),
            Field::new("lat_e7", DataType::Int32, false),
        ],
        &pts,
        |rows| {
            vec![
                Arc::new(Int32Array::from_iter_values(rows.iter().map(|p| p.0))) as ArrayRef,
                Arc::new(Int32Array::from_iter_values(rows.iter().map(|p| p.1))) as ArrayRef,
            ]
        },
    )?;
    let mut summary = format!("# buildings: {id}\n\n");
    let _ = writeln!(
        summary,
        "{} buildings inside {extent} of {read} read ({reader}), in {:.1?}.",
        pts.len(),
        t.elapsed()
    );
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

/// Reads the points of one gzipped line file: (points inside the extent, lines read).
fn gz_lines(
    path: &Path,
    ext: Extent,
    parse: fn(&str) -> Result<Option<Pt>, String>,
) -> Result<(Vec<Pt>, u64), String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = BufReader::with_capacity(1 << 20, MultiGzDecoder::new(file));
    let (mut out, mut read, mut line) = (Vec::new(), 0_u64, String::new());
    loop {
        line.clear();
        let n = lines
            .read_line(&mut line)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        let text = line.trim_end();
        if text.is_empty() {
            continue;
        }
        read += 1;
        let p = parse(text).map_err(|e| format!("{}: line {read}: {e}", path.display()))?;
        if let Some(p) = p.filter(|&p| ext.holds(p)) {
            out.push(p);
        }
    }
    Ok((out, read))
}

/// One Open Buildings v3 CSV row: `lat,lon,area,confidence,plus_code`.
fn google_line(line: &str) -> Result<Option<Pt>, String> {
    let mut f = line.split(',');
    let (Some(lat), Some(lon)) = (f.next(), f.next()) else {
        return Err("expected latitude,longitude,…".into());
    };
    let deg = |v: &str| v.trim().parse::<f64>().map_err(|e| format!("`{v}`: {e}"));
    let (lat, lon) = (deg(lat)?, deg(lon)?);
    Ok(e7(lon).zip(e7(lat)))
}

#[derive(Deserialize)]
struct Feature {
    geometry: Geometry,
}

#[derive(Deserialize)]
#[serde(tag = "type", content = "coordinates")]
enum Geometry {
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

/// One Microsoft `GeoJSON` line: the vertex mean of the (first) outer ring.
fn ms_line(line: &str) -> Result<Option<Pt>, String> {
    let f: Feature = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let ring = match &f.geometry {
        Geometry::Polygon(rings) => rings.first(),
        Geometry::MultiPolygon(polys) => polys.first().and_then(|p| p.first()),
    };
    let Some(ring) = ring else { return Ok(None) };
    let open = match ring.as_slice() {
        // Exact equality on purpose: a closed ring repeats its first vertex bit for bit.
        [first, .., last] if first.map(f64::to_bits) == last.map(f64::to_bits) => {
            ring.get(..ring.len() - 1).unwrap_or_default()
        }
        _ => ring.as_slice(),
    };
    if open.is_empty() {
        return Ok(None);
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "a ring has far fewer than 2^52 vertices"
    )]
    let n = open.len() as f64;
    let (sx, sy) = open
        .iter()
        .fold((0.0, 0.0), |(x, y), c| (x + c[0], y + c[1]));
    Ok(e7(sx / n).zip(e7(sy / n)))
}

fn is_building(t: (&str, &str)) -> bool {
    t.0 == "building" && t.1 != "no"
}

/// OSM buildings: (points inside the extent, building ways read).
fn osm(path: &Path, ext: Extent) -> Result<(Vec<Pt>, u64), String> {
    let open = || ElementReader::from_path(path).map_err(|e| format!("{}: {e}", path.display()));
    // Pass 1: each building way's first node.
    let mut firsts: Vec<i64> = open()?
        .par_map_reduce(
            |el| match el {
                Element::Way(w) if w.tags().any(is_building) => {
                    w.refs().next().into_iter().collect()
                }
                _ => Vec::new(),
            },
            Vec::new,
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
    firsts.par_sort_unstable();
    let ways = firsts.len() as u64;
    let mut wanted = firsts.clone();
    wanted.dedup();
    // Pass 2: the locations of those nodes.
    let keep = |id: i64, lat: i32, lon: i32| -> Vec<(i64, Pt)> {
        if wanted.binary_search(&id).is_ok() {
            vec![(id, (lon, lat))]
        } else {
            Vec::new()
        }
    };
    let mut located: Vec<(i64, Pt)> = open()?
        .par_map_reduce(
            |el| match el {
                Element::Node(n) => keep(n.id(), n.decimicro_lat(), n.decimicro_lon()),
                Element::DenseNode(n) => keep(n.id(), n.decimicro_lat(), n.decimicro_lon()),
                _ => Vec::new(),
            },
            Vec::new,
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
    located.par_sort_unstable_by_key(|l| l.0);
    // One point per building way (two ways may start at one node).
    let pts = firsts
        .iter()
        .filter_map(|id| {
            let i = located.binary_search_by_key(id, |l| l.0).ok()?;
            located.get(i).map(|l| l.1)
        })
        .filter(|&p| ext.holds(p))
        .collect();
    Ok((pts, ways))
}

#[cfg(test)]
mod tests {
    use super::{Extent, google_line, ms_line};

    #[test]
    fn lines_give_points() {
        assert_eq!(
            google_line("27.7172453,85.3239605,41.2,0.81,7MV7P8C7+V5"),
            Ok(Some((853_239_605, 277_172_453)))
        );
        assert!(google_line("27.7,x,1,1,p").is_err());
        assert!(google_line("").is_err());
        // The closing vertex does not count twice: a unit square's mean is its centre.
        let sq = r#"{"type":"Feature","properties":{"height":-1.0,"confidence":-1.0},"geometry":{"type":"Polygon","coordinates":[[[85.0,27.0],[85.0001,27.0],[85.0001,27.0001],[85.0,27.0001],[85.0,27.0]]]}}"#;
        assert_eq!(ms_line(sq), Ok(Some((850_000_500, 270_000_500))));
        let multi = r#"{"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[1.0,2.0],[1.0,2.0002],[1.0002,2.0]]]]}}"#;
        assert!(ms_line(multi).is_ok_and(|p| p.is_some()));
        assert!(ms_line(r#"{"geometry":{"type":"Point","coordinates":[1,2]}}"#).is_err());
        let e = Extent::parse("80,26,88.5,30.5").expect("extent");
        assert!(e.holds((850_000_000, 270_000_000)) && !e.holds((790_000_000, 270_000_000)));
        assert!(Extent::parse("88,26,80,30").is_err() && Extent::parse("1,2,3").is_err());
    }
}
