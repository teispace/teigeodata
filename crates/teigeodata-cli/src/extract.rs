//! `extract`: a smaller pack from a pack.
//!
//! - **By component:** the sections of the closed set (each component with what it needs),
//!   copied as stored: same bytes, same hashes, so a store could install either.
//! - **By box:** the places whose point is in the box, with every ancestor (so each kept place
//!   keeps its chain, D-143) and its timezone area (so it keeps its zone, D-151), re-encoded by
//!   `teigeodata_format::extract` (D-145), boundaries at level of detail `lod`. Terrain keeps
//!   the tiles that touch the box, copied as stored (`dem::mask`); REGION, POSTAL, REDIRECTS, COMPONENTS and BLOBS are
//!   keyed by rows the slice renumbers, so they are left out and named on standard error.
//!
//! The manifest is the source's (its attribution above all), unsigned, with the component
//! renamed: a pack's identity is its address, component and release, and an extract is not
//! the pack it came from.

use std::io::Write;
use std::path::Path;

use teigeodata_format::component::{Component, Components};
use teigeodata_format::dem::{self, Dem, Grid};
use teigeodata_format::extract;
use teigeodata_format::hierarchy::Hierarchy;
use teigeodata_format::place_tz::PlaceTz;
use teigeodata_format::places::Places;
use teigeodata_format::region::Options as RegionOptions;
use teigeodata_format::{Encoding, Kind, Pack, Part, Tag, Writer};

use crate::Fail;
use crate::pack::{open, read};

/// A box on the 1e-6° grid; `west > east` crosses the antimeridian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bbox {
    west: i32,
    south: i32,
    east: i32,
    north: i32,
}

impl Bbox {
    /// Whether tile (`row`, `col`) of `grid`'s global tile grid overlaps the box widened by
    /// two samples on each side, so a height read by interpolation at the box's edge finds
    /// its neighbours.
    #[allow(
        clippy::cast_precision_loss,
        reason = "tile edges in degrees, well within f64"
    )]
    fn touches_tile(self, grid: &Grid, row: u32, col: u32) -> bool {
        // Steps are in 0.1″: degrees = step / 36,000. Rows count down from 90°N, columns
        // east from 180°W.
        let side = grid.side() as f64;
        let lat_step = f64::from(grid.lat_step) / 36_000.0;
        let lon_step = f64::from(grid.lon_step) / 36_000.0;
        let north = 90.0 - f64::from(row) * side * lat_step;
        let south = north - side * lat_step;
        let west = -180.0 + f64::from(col) * side * lon_step;
        let east = west + side * lon_step;
        let (pad_lat, pad_lon) = (2.0 * lat_step, 2.0 * lon_step);
        let deg = |v: i32| f64::from(v) / 1e6;
        let lat_in = south <= deg(self.north) + pad_lat && north >= deg(self.south) - pad_lat;
        let overlaps = |w: f64, e: f64| west <= e + pad_lon && east >= w - pad_lon;
        let lon_in = if self.west <= self.east {
            overlaps(deg(self.west), deg(self.east))
        } else {
            overlaps(deg(self.west), 180.0) || overlaps(-180.0, deg(self.east))
        };
        lat_in && lon_in
    }

    fn holds(self, lat: i32, lon: i32) -> bool {
        let lon_in = if self.west <= self.east {
            self.west <= lon && lon <= self.east
        } else {
            lon >= self.west || lon <= self.east
        };
        self.south <= lat && lat <= self.north && lon_in
    }
}

/// `west,south,east,north` in degrees.
pub(crate) fn parse_bbox(text: &str) -> Result<Bbox, String> {
    let v: Vec<f64> = text
        .split(',')
        .map(|x| x.trim().parse::<f64>().map_err(|e| format!("{x:?}: {e}")))
        .collect::<Result<_, _>>()?;
    let [west, south, east, north] = v[..] else {
        return Err("expected four numbers: west,south,east,north".into());
    };
    let grid = |d: f64, max: f64| {
        if d.is_finite() && d.abs() <= max {
            #[allow(clippy::cast_possible_truncation, reason = "within ±180e6")]
            Ok((d * 1e6).round() as i32)
        } else {
            Err(format!("{d} is outside ±{max}"))
        }
    };
    let bbox = Bbox {
        west: grid(west, 180.0)?,
        south: grid(south, 90.0)?,
        east: grid(east, 180.0)?,
        north: grid(north, 90.0)?,
    };
    if bbox.south > bbox.north {
        return Err("south is above north".into());
    }
    Ok(bbox)
}

/// What to keep.
#[derive(Debug)]
pub(crate) struct Options<'a> {
    pub(crate) components: &'a [Component],
    pub(crate) bbox: Option<Bbox>,
    pub(crate) lod: u8,
    pub(crate) name: &'a str,
}

/// The rows to keep for `bbox`: in it, their ancestors, their timezone areas; increasing.
fn rows_in(pack: &Pack<'_>, bbox: Bbox) -> Result<Vec<u32>, teigeodata_format::Error> {
    let places = match pack.section(Kind::PLACES) {
        Some(s) => Places::open(&s)?,
        None => return Ok(Vec::new()),
    };
    let hierarchy = pack
        .section(Kind::HIERARCHY)
        .map(|s| Hierarchy::open(&s))
        .transpose()?;
    let zones = pack
        .section(Kind::PLACE_TZ)
        .map(|s| PlaceTz::open(&s))
        .transpose()?;
    let mut keep = vec![false; places.len()];
    let mark = |row: usize, keep: &mut Vec<bool>| {
        // Each ancestor once: a chain stops at a row already kept.
        let mut at = Some(row);
        while let Some(r) = at {
            match keep.get_mut(r) {
                Some(k) if !*k => *k = true,
                _ => break,
            }
            at = hierarchy.as_ref().and_then(|h| h.parent(r));
        }
    };
    for row in 0..places.len() {
        if let Some(p) = places.point(row)
            && bbox.holds(p.lat(), p.lon())
        {
            mark(row, &mut keep);
            if let Some(z) = zones.as_ref().and_then(|z| z.get(row)) {
                mark(z as usize, &mut keep);
            }
        }
    }
    Ok(keep
        .iter()
        .enumerate()
        .filter(|(_, k)| **k)
        .filter_map(|(r, _)| u32::try_from(r).ok())
        .collect())
}

/// The new pack's writer: the source's manifest, its component renamed, its box the one cut.
fn writer(pack: &Pack<'_>, opts: &Options<'_>) -> Result<Writer, teigeodata_format::Error> {
    let m = pack.manifest();
    let mut w = Writer::new(m.address(), opts.name, m.release())?;
    for (tag, value) in m.entries() {
        if ![Tag::ADDRESS, Tag::COMPONENT, Tag::RELEASE, Tag::BBOX].contains(&tag) {
            w.manifest(tag, value)?;
        }
    }
    // The box of what is kept, when cut by a box; the source's otherwise.
    if let Some(b) = m.bbox() {
        let b = match opts.bbox {
            Some(x) => teigeodata_format::Bbox {
                min: teigeodata_core::Point::from_degrees(
                    f64::from(x.south) / 1e6,
                    f64::from(x.west) / 1e6,
                )
                .map_err(|_| teigeodata_format::Error::Write("the box is off the grid"))?,
                max: teigeodata_core::Point::from_degrees(
                    f64::from(x.north) / 1e6,
                    f64::from(x.east) / 1e6,
                )
                .map_err(|_| teigeodata_format::Error::Write("the box is off the grid"))?,
            },
            None => b,
        };
        w.manifest(Tag::BBOX, &b.to_bytes())?;
    }
    Ok(w)
}

pub(crate) fn run(
    from: &Path,
    to: &Path,
    opts: &Options<'_>,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    if opts.components.is_empty() && opts.bbox.is_none() {
        return Err(Fail::Refused(
            "nothing to extract: give --components, --bbox or both".into(),
        ));
    }
    let bytes = read(from)?;
    let pack = open(from, &bytes)?;
    pack.verify_sections()
        .map_err(|e| Fail::of(from.display(), e))?;
    let bad = |e| Fail::of(from.display(), e);
    let set = if opts.components.is_empty() {
        Components::all()
    } else {
        Components::of(opts.components).closed()
    };
    // Each kept section with its part of its kind (ADR-0040): a pack may hold several parts of
    // one kind (names by language, outlines by level of detail).
    let mut bodies: Vec<(Kind, Part, Encoding, u64, Vec<u8>)> = match opts.bbox {
        Some(b) => {
            let keep = rows_in(&pack, b).map_err(bad)?;
            let mut v: Vec<_> = extract::rows(&pack, &keep, opts.lod, &RegionOptions::default())
                .map_err(bad)?
                .into_iter()
                .map(|(k, e, n, body)| (k, Part::BASE, e, n, body))
                .collect();
            // Terrain is keyed by coordinates: the tiles that touch the box are kept, codes
            // copied as stored, so every kept sample reads back exactly.
            for kind in [Kind::DEM, Kind::DEM_COARSE] {
                if let Some(section) = pack.section(kind) {
                    let dem = Dem::open(&section).map_err(bad)?;
                    if let Some((body, n)) =
                        dem::mask(&dem, |g, r, c| b.touches_tile(g, r, c)).map_err(bad)?
                    {
                        v.push((kind, Part::BASE, section.encoding(), n, body));
                    }
                }
            }
            v
        }
        // Every section as it is, but the source's block hashes: the writer hashes what it
        // writes, and a second BLOCK_HASHES would be a kind given twice.
        None => pack
            .sections()
            .filter(|s| s.kind() != Kind::BLOCK_HASHES)
            .map(|s| {
                (
                    s.kind(),
                    s.part(),
                    s.encoding(),
                    s.item_count(),
                    s.bytes().to_vec(),
                )
            })
            .collect(),
    };
    let had: Vec<Kind> = pack.sections().map(|s| s.kind()).collect();
    bodies.retain(|b| set.holds(b.0));
    let mut left_out: Vec<String> = had
        .iter()
        .filter(|k| **k != Kind::BLOCK_HASHES && set.holds(**k))
        .filter(|k| !bodies.iter().any(|b| b.0 == **k))
        .map(ToString::to_string)
        .collect();
    left_out.dedup();
    let w = writer(&pack, opts).map_err(|e| Fail::of(from.display(), e))?;
    let refs: Vec<(Kind, Part, Encoding, u64, &[u8])> = bodies
        .iter()
        .map(|(k, p, en, n, b)| (*k, *p, *en, *n, b.as_slice()))
        .collect();
    let file = w.pack_parts(&refs, None).map_err(bad)?;
    // Written beside the target, then renamed: a cut-off run leaves no half pack.
    let tmp = to.with_extension("tgd.part");
    std::fs::write(&tmp, &file).map_err(|e| Fail::of(tmp.display(), e))?;
    std::fs::rename(&tmp, to).map_err(|e| Fail::of(to.display(), e))?;
    let kept = Pack::open(&file)
        .ok()
        .and_then(|p| p.section(Kind::PLACES).map(|s| s.item_count()))
        .unwrap_or(0);
    writeln!(
        out,
        "wrote {}: {} bytes, {} sections, {kept} places; components {}",
        to.display(),
        file.len(),
        refs.len(),
        set.iter()
            .map(Component::name)
            .collect::<Vec<_>>()
            .join(",")
    )?;
    if !left_out.is_empty() {
        let _ = writeln!(
            std::io::stderr(),
            "tgd: left out (keyed by rows a box renumbers): {}",
            left_out.join(", ")
        );
    }
    Ok(())
}
