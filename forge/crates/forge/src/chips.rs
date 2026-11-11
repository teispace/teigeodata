//! `forge labels chips|set`: the images a labeller marks and the labels they give
//! ([accuracy labels](../../../../docs/02-research/notes/accuracy-labels.md), D-209).
//!
//! ```text
//! forge labels chips <cc> [--pass 2] [--only <id,…>]
//! forge labels set <cc> <picks.tsv> <labeller> [--pass 2]
//! ```
//!
//! - `chips` draws one PNG per row of `tests/golden/<cc>-places.csv` (pass 2: the rows marked
//!   `second`) into `data/build/labels/<cc>/pass<n>/`: Copernicus Sentinel-2 L2A true colour
//!   (10 m) around the row's locator, 2 km square (4 km for a town), the raw building points of
//!   the three footprint sources on top and a metric grid, and `chips.tsv` with each chip's
//!   transform. The scene is the least cloudy of the dry seasons whose data covers the whole
//!   chip; it is read by HTTP range, only the internal tiles the chip meets. Pass 2 mirrors each
//!   chip and moves its centre, so a second labeller cannot reuse pixel positions. Nothing from
//!   the forge's own outputs is drawn.
//! - `set` reads picks (`id`, `x`, `y`, `notes`; `-` for no visible settlement) in a chip's
//!   pixels and writes the label, the labeller and the notes: pass 1 into the sample's own
//!   columns, pass 2 into `tests/golden/<cc>-places-pass2.csv`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use arrow_array::Int32Array;
use tiff::decoder::DecodingResult;
use tiff::tags::Tag;

use crate::labels::{field, split};
use crate::raster;
use crate::record::{column, for_each_batch};

/// Earth Search, the STAC API over the AWS open-data Sentinel-2 COGs.
const STAC: &str = "https://earth-search.aws.element84.com/v1/search";
/// Dry seasons, newest first: clear skies over South Asia (the scene order inside one is by
/// cloud).
const SEASONS: [&str; 3] = [
    "2025-10-01T00:00:00Z/2026-02-28T23:59:59Z",
    "2024-10-01T00:00:00Z/2025-02-28T23:59:59Z",
    "2023-10-01T00:00:00Z/2024-02-28T23:59:59Z",
];
/// Scenes tried per chip before it is reported as not drawn.
const TRIES: usize = 8;
/// Output chip side in pixels.
const SIZE: usize = 600;
/// Half the chip's side: a town's is twice a village's.
const HALF_M: f64 = 1000.0;
/// Pass 2's shift of the chip centre (east, north), metres.
const SHIFT_M: (f64, f64) = (237.0, -173.0);
/// HTTP range block.
const BLOCK: u64 = 1 << 20;
/// A building point's colour (magenta) and the grid's (white).
const DOT: [u8; 3] = [255, 0, 255];
const GRID: [u8; 3] = [255, 255, 255];
/// The building points' sources, as `forge buildings` wrote them.
const BUILDINGS: [&str; 3] = ["google-buildings-v3", "msbuildings", "geofabrik"];
/// 1e-7 degrees.
const E7: f64 = 1e7;
/// Half the locator's 0.01° cell.
const LOCATOR_HALF_CELL: f64 = 0.005;

// ---------- UTM (Krüger series to order n⁴, Karney 2011; WGS 84) ----------

const A_WGS84: f64 = 6_378_137.0;
const F_WGS84: f64 = 1.0 / 298.257_223_563;
const K0: f64 = 0.9996;
const FALSE_E: f64 = 500_000.0;

struct Utm {
    /// Central meridian, radians.
    lon0: f64,
    false_n: f64,
    a_hat: f64,
    alpha: [f64; 4],
    beta: [f64; 4],
    delta: [f64; 4],
    e_term: f64,
}

impl Utm {
    /// The projection of EPSG `326zz` (north) or `327zz` (south).
    fn of_epsg(epsg: u32) -> Option<Utm> {
        let (zone, south) = match epsg {
            32601..=32660 => (epsg - 32600, false),
            32701..=32760 => (epsg - 32700, true),
            _ => return None,
        };
        let n = F_WGS84 / (2.0 - F_WGS84);
        let (n2, n3, n4) = (n * n, n * n * n, n * n * n * n);
        Some(Utm {
            lon0: (f64::from(zone) * 6.0 - 183.0).to_radians(),
            false_n: if south { 10_000_000.0 } else { 0.0 },
            a_hat: A_WGS84 / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0),
            alpha: [
                n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0,
                13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0,
                61.0 * n3 / 240.0 - 103.0 * n4 / 140.0,
                49_561.0 * n4 / 161_280.0,
            ],
            beta: [
                n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0,
                n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0,
                17.0 * n3 / 480.0 - 37.0 * n4 / 840.0,
                4397.0 * n4 / 161_280.0,
            ],
            delta: [
                2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0,
                7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0,
                56.0 * n3 / 15.0 - 136.0 * n4 / 35.0,
                4279.0 * n4 / 630.0,
            ],
            e_term: 2.0 * n.sqrt() / (1.0 + n),
        })
    }

    /// (lon, lat) degrees → (easting, northing) metres.
    fn forward(&self, (lon, lat): (f64, f64)) -> (f64, f64) {
        let (phi, lam) = (lat.to_radians(), lon.to_radians() - self.lon0);
        let s = phi.sin();
        let t = (s.atanh() - self.e_term * (self.e_term * s).atanh()).sinh();
        let xi1 = t.atan2(lam.cos());
        let eta1 = (lam.sin() / t.hypot(1.0)).atanh();
        let (mut xi, mut eta) = (xi1, eta1);
        for (j, a) in (1_u8..).zip(self.alpha) {
            let k = 2.0 * f64::from(j);
            xi += a * (k * xi1).sin() * (k * eta1).cosh();
            eta += a * (k * xi1).cos() * (k * eta1).sinh();
        }
        (
            FALSE_E + K0 * self.a_hat * eta,
            self.false_n + K0 * self.a_hat * xi,
        )
    }

    /// (easting, northing) metres → (lon, lat) degrees.
    fn inverse(&self, (e, n): (f64, f64)) -> (f64, f64) {
        let xi = (n - self.false_n) / (K0 * self.a_hat);
        let eta = (e - FALSE_E) / (K0 * self.a_hat);
        let (mut xi1, mut eta1) = (xi, eta);
        for (j, b) in (1_u8..).zip(self.beta) {
            let k = 2.0 * f64::from(j);
            xi1 -= b * (k * xi).sin() * (k * eta).cosh();
            eta1 -= b * (k * xi).cos() * (k * eta).sinh();
        }
        let chi = (xi1.sin() / eta1.cosh()).asin();
        let mut phi = chi;
        for (j, d) in (1_u8..).zip(self.delta) {
            phi += d * (2.0 * f64::from(j) * chi).sin();
        }
        let lam = eta1.sinh().atan2(xi1.cos());
        ((self.lon0 + lam).to_degrees(), phi.to_degrees())
    }
}

// ---------- HTTP ranges as a seekable reader ----------

/// A remote file read by HTTP `Range`, one block at a time, the blocks it read kept.
struct Ranged {
    url: String,
    pos: u64,
    len: u64,
    blocks: HashMap<u64, Vec<u8>>,
}

impl Ranged {
    fn open(url: &str) -> Result<Ranged, String> {
        let mut r = Ranged {
            url: url.to_string(),
            pos: 0,
            len: u64::MAX,
            blocks: HashMap::new(),
        };
        r.block(0).map_err(|e| format!("{url}: {e}"))?;
        Ok(r)
    }

    /// Block `b` (fetched once), and the file length from its `Content-Range`.
    fn block(&mut self, b: u64) -> std::io::Result<&[u8]> {
        if !self.blocks.contains_key(&b) {
            let (start, end) = (b * BLOCK, b * BLOCK + BLOCK - 1);
            let io = |e: String| std::io::Error::other(e);
            let mut resp = ureq::get(&self.url)
                .header("Range", &format!("bytes={start}-{end}"))
                .header("User-Agent", crate::fetch::USER_AGENT)
                .call()
                .map_err(|e| io(format!("{}: {e}", self.url)))?;
            if let Some(total) = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse::<u64>().ok())
            {
                self.len = total;
            }
            let bytes = resp
                .body_mut()
                .with_config()
                .limit(BLOCK + 1)
                .read_to_vec()
                .map_err(|e| io(format!("{}: {e}", self.url)))?;
            self.blocks.insert(b, bytes);
        }
        Ok(self.blocks.get(&b).map_or(&[][..], Vec::as_slice))
    }
}

impl Read for Ranged {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let (b, off) = (self.pos / BLOCK, self.pos % BLOCK);
        let data = self.block(b)?;
        let off = usize::try_from(off).map_err(std::io::Error::other)?;
        let Some(rest) = data.get(off..) else {
            return Ok(0);
        };
        let n = rest.len().min(buf.len());
        if let (Some(dst), Some(src)) = (buf.get_mut(..n), rest.get(..n)) {
            dst.copy_from_slice(src);
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Ranged {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let next = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
        };
        self.pos = next.ok_or_else(|| std::io::Error::other("seek before the start"))?;
        Ok(self.pos)
    }
}

// ---------- the chip ----------

/// One row of the sample.
struct Row {
    id: String,
    name: String,
    class: String,
    second: bool,
    /// The middle of the locator's cell (lon, lat), degrees.
    at: (f64, f64),
}

/// A drawn chip's frame: its centre (easting, northing), half side, mirror, EPSG.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    epsg: u32,
    centre: (f64, f64),
    half: f64,
    mirror: bool,
}

impl Frame {
    /// Metres per output pixel.
    fn mpp(&self) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "the chip's side in pixels")]
        let size = SIZE as f64;
        2.0 * self.half / size
    }

    /// The centre of output pixel `(u, v)`, as (easting, northing).
    fn at(&self, (u, v): (f64, f64)) -> (f64, f64) {
        let m = self.mpp();
        let du = (u + 0.5) * m - self.half;
        let e = if self.mirror {
            self.centre.0 - du
        } else {
            self.centre.0 + du
        };
        (e, self.centre.1 + self.half - (v + 0.5) * m)
    }

    /// The output pixel (fractional) of a point (easting, northing).
    fn pixel(&self, (e, n): (f64, f64)) -> (f64, f64) {
        let m = self.mpp();
        let du = if self.mirror {
            self.centre.0 - e
        } else {
            e - self.centre.0
        };
        (
            (du + self.half) / m - 0.5,
            (self.centre.1 + self.half - n) / m - 0.5,
        )
    }
}

/// A scene candidate: id, date, cloud, TCI URL.
struct Scene {
    id: String,
    date: String,
    cloud: f64,
    url: String,
}

/// Scenes over a box (degrees), each dry season's least cloudy first.
fn scenes(bbox: (f64, f64, f64, f64)) -> Result<Vec<Scene>, String> {
    let mut out = Vec::new();
    for season in SEASONS {
        let body = serde_json::json!({
            "collections": ["sentinel-2-l2a"],
            "bbox": [bbox.0, bbox.1, bbox.2, bbox.3],
            "datetime": season,
            "query": {"eo:cloud_cover": {"lt": 20}},
            "limit": 50,
        })
        .to_string();
        let text = ureq::post(STAC)
            .header("Content-Type", "application/json")
            .header("User-Agent", crate::fetch::USER_AGENT)
            .send(&body)
            .and_then(|mut r| r.body_mut().read_to_string())
            .map_err(|e| format!("{STAC}: {e}"))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{STAC}: {e}"))?;
        let mut these: Vec<Scene> = v
            .get("features")
            .and_then(serde_json::Value::as_array)
            .map(|f| {
                f.iter()
                    .filter_map(|s| {
                        Some(Scene {
                            id: s.get("id")?.as_str()?.to_string(),
                            date: s
                                .pointer("/properties/datetime")?
                                .as_str()?
                                .get(..10)?
                                .to_string(),
                            cloud: s.pointer("/properties/eo:cloud_cover")?.as_f64()?,
                            url: s.pointer("/assets/visual/href")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        these.sort_by(|a, b| {
            a.cloud
                .total_cmp(&b.cloud)
                .then_with(|| b.date.cmp(&a.date))
        });
        out.extend(these);
    }
    Ok(out)
}

/// The chip of `frame` (centre given in degrees, projected here) from one scene, as RGB rows
/// before the overlay; `None` when the scene does not cover it with data.
fn draw(
    scene: &Scene,
    at: (f64, f64),
    half: f64,
    mirror: bool,
    shift: (f64, f64),
) -> Result<Option<(Frame, Vec<u8>)>, String> {
    let what = &scene.url;
    let (mut dec, grid) = raster::open_reader(Ranged::open(what)?, what)?;
    let keys = dec
        .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
        .unwrap_or_default();
    let epsg = keys
        .chunks(4)
        .skip(1)
        .find(|k| k.first() == Some(&3072))
        .and_then(|k| k.get(3))
        .map(|&c| u32::from(c))
        .ok_or_else(|| format!("{what}: no projected CRS"))?;
    let utm = Utm::of_epsg(epsg).ok_or_else(|| format!("{what}: EPSG {epsg} is not UTM"))?;
    let centre = utm.forward(at);
    let frame = Frame {
        epsg,
        centre: (centre.0 + shift.0, centre.1 + shift.1),
        half,
        mirror,
    };
    // Scene pixel (column, row) of a point: `g.lon0`, `g.lat0` are the first pixel's centre.
    let col = |east: f64| (east - grid.lon0) / grid.dx;
    let row = |north: f64| (grid.lat0 - north) / grid.dy;
    #[allow(clippy::cast_precision_loss, reason = "a scene's pixel count")]
    let (wf, hf) = (grid.w as f64, grid.h as f64);
    let (c0, c1) = (col(frame.centre.0 - half), col(frame.centre.0 + half));
    let (r0, r1) = (row(frame.centre.1 + half), row(frame.centre.1 - half));
    if c0 < 0.0 || r0 < 0.0 || c1 >= wf - 1.0 || r1 >= hf - 1.0 {
        return Ok(None);
    }
    let (cw, ch) = dec.chunk_dimensions();
    let (cw, ch) = (cw as usize, ch as usize);
    if cw == 0 || ch == 0 {
        return Err(format!("{what}: empty chunks"));
    }
    let across = grid.w.div_ceil(cw);
    let mut tiles: HashMap<usize, (usize, Vec<u8>)> = HashMap::new();
    let mut rgb = vec![0_u8; SIZE * SIZE * 3];
    let mut empty = 0_usize;
    for v in 0..SIZE {
        for u in 0..SIZE {
            #[allow(clippy::cast_precision_loss, reason = "pixel indices")]
            let (east, north) = frame.at((u as f64, v as f64));
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "inside the scene, checked above"
            )]
            let (ci, rj) = (col(east).round() as usize, row(north).round() as usize);
            let index = (rj / ch) * across + ci / cw;
            if let std::collections::hash_map::Entry::Vacant(slot) = tiles.entry(index) {
                let chunk = u32::try_from(index).map_err(|e| e.to_string())?;
                let width = dec.chunk_data_dimensions(chunk).0 as usize;
                let DecodingResult::U8(data) =
                    dec.read_chunk(chunk).map_err(|e| format!("{what}: {e}"))?
                else {
                    return Err(format!("{what}: not an 8-bit raster"));
                };
                slot.insert((width, data));
            }
            let px = tiles.get(&index).and_then(|(width, data)| {
                let start = ((rj % ch) * width + ci % cw) * 3;
                data.get(start..start + 3)
            });
            let out = (v * SIZE + u) * 3;
            match (px, rgb.get_mut(out..out + 3)) {
                (Some(src), Some(dst)) => {
                    dst.copy_from_slice(src);
                    empty += usize::from(src == [0, 0, 0]);
                }
                _ => empty += 1,
            }
        }
    }
    // A chip on the scene's edge of data (black) is drawn from another scene.
    if empty * 50 > SIZE * SIZE {
        return Ok(None);
    }
    stretch(&mut rgb);
    Ok(Some((frame, rgb)))
}

/// Stretches each channel from its 2nd to its 98th percentile, for hills in shadow.
fn stretch(rgb: &mut [u8]) {
    for ch in 0..3 {
        let mut hist = [0_usize; 256];
        for p in rgb.as_chunks::<3>().0 {
            if let Some(&v) = p.get(ch)
                && let Some(h) = hist.get_mut(usize::from(v))
            {
                *h += 1;
            }
        }
        let total: usize = hist.iter().sum();
        let pct = |q: usize| {
            let mut acc = 0;
            for (v, &h) in hist.iter().enumerate() {
                acc += h;
                if acc * 100 >= total * q {
                    return v;
                }
            }
            255
        };
        let (lo, hi) = (pct(2), pct(98).max(pct(2) + 1));
        for p in rgb.as_chunks_mut::<3>().0 {
            if let Some(v) = p.get_mut(ch) {
                let x = usize::from(*v).clamp(lo, hi) - lo;
                *v = u8::try_from(x * 255 / (hi - lo)).unwrap_or(u8::MAX);
            }
        }
    }
}

/// Blends `colour` over pixel `(u, v)` by `alpha` of 256.
fn blend(rgb: &mut [u8], (u, v): (usize, usize), colour: [u8; 3], alpha: u16) {
    if u >= SIZE || v >= SIZE {
        return;
    }
    let at = (v * SIZE + u) * 3;
    if let Some(p) = rgb.get_mut(at..at + 3) {
        for (c, k) in p.iter_mut().zip(colour) {
            let mixed = (u16::from(*c) * (256 - alpha) + u16::from(k) * alpha) / 256;
            *c = u8::try_from(mixed).unwrap_or(u8::MAX);
        }
    }
}

/// The building points (easting, northing in the frame's zone) and the metric grid.
fn overlay(rgb: &mut [u8], frame: &Frame, points: &[(f64, f64)]) {
    let step = if frame.half > HALF_M { 250.0 } else { 100.0 };
    let metres_per_px = frame.mpp();
    // Grid lines at whole multiples of `step` metres east and north; every fifth stronger.
    let (e0, n0) = (frame.centre.0 - frame.half, frame.centre.1 - frame.half);
    let mut k = (e0 / step).ceil();
    while k * step <= e0 + 2.0 * frame.half {
        let (at, _) = frame.pixel((k * step, frame.centre.1));
        #[allow(clippy::cast_possible_truncation, reason = "a fraction of the chip")]
        let strong = (k as i64).rem_euclid(5) == 0;
        line(rgb, at, true, if strong { 110 } else { 50 });
        k += 1.0;
    }
    let mut k = (n0 / step).ceil();
    while k * step <= n0 + 2.0 * frame.half {
        let (_, at) = frame.pixel((frame.centre.0, k * step));
        #[allow(clippy::cast_possible_truncation, reason = "a fraction of the chip")]
        let strong = (k as i64).rem_euclid(5) == 0;
        line(rgb, at, false, if strong { 110 } else { 50 });
        k += 1.0;
    }
    let radius = usize::from(metres_per_px <= 4.0);
    #[allow(clippy::cast_precision_loss, reason = "the chip's side in pixels")]
    let side = SIZE as f64;
    for &p in points {
        let (u, v) = frame.pixel(p);
        if !(0.0..side).contains(&u) || !(0.0..side).contains(&v) {
            continue;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "inside the chip, checked above"
        )]
        let (u, v) = (u as usize, v as usize);
        for dv in 0..=radius {
            for du in 0..=radius {
                blend(rgb, (u + du, v + dv), DOT, 170);
            }
        }
    }
}

/// A full-height (or full-width) grid line at pixel `at`.
fn line(rgb: &mut [u8], at: f64, vertical: bool, alpha: u16) {
    #[allow(clippy::cast_precision_loss, reason = "the chip's side in pixels")]
    let side = SIZE as f64;
    if !(0.0..side).contains(&at) {
        return;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "inside the chip, checked above"
    )]
    let at = at.round() as usize;
    for t in 0..SIZE {
        blend(rgb, if vertical { (at, t) } else { (t, at) }, GRID, alpha);
    }
}

/// An RGB image as PNG: one `IDAT`, rows unfiltered, zlib.
fn png(rgb: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| -> Result<(), String> {
        let len = u32::try_from(data.len()).map_err(|e| e.to_string())?;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
        Ok(())
    };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(w).map_err(|e| e.to_string())?.to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(h).map_err(|e| e.to_string())?.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr)?;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    for row in rgb.chunks_exact(w * 3).take(h) {
        z.write_all(&[0]).map_err(|e| e.to_string())?;
        z.write_all(row).map_err(|e| e.to_string())?;
    }
    chunk(&mut out, b"IDAT", &z.finish().map_err(|e| e.to_string())?)?;
    chunk(&mut out, b"IEND", &[])?;
    Ok(out)
}

// ---------- the commands ----------

fn sample_path(root: &Path, cc: &str) -> PathBuf {
    root.join("tests/golden").join(format!("{cc}-places.csv"))
}

fn pass2_path(root: &Path, cc: &str) -> PathBuf {
    root.join("tests/golden")
        .join(format!("{cc}-places-pass2.csv"))
}

fn rows(root: &Path, cc: &str) -> Result<Vec<Row>, String> {
    let path = sample_path(root, cc);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate().skip(1) {
        let f = split(line);
        let get = |i: usize| f.get(i).map_or("", String::as_str);
        let num = |i: usize| {
            get(i)
                .parse::<f64>()
                .map_err(|e| format!("{}:{}: {e}", path.display(), n + 1))
        };
        out.push(Row {
            id: get(0).to_string(),
            name: get(1).to_string(),
            class: get(2).to_string(),
            second: get(6) == "yes",
            // The sample truncates the source point to 0.01° (`labels::sample`), so the place
            // lies in the cell north-east of the locator: its middle is the chip's centre.
            at: (num(5)? + LOCATOR_HALF_CELL, num(4)? + LOCATOR_HALF_CELL),
        });
    }
    Ok(out)
}

/// The building points inside each box (lon, lat in 1e-7°), streamed from the three sources.
fn buildings(
    root: &Path,
    cc: &str,
    boxes: &[(i32, i32, i32, i32)],
) -> Result<Vec<Vec<(i32, i32)>>, String> {
    // Boxes by 0.1° cell, so each point is tested against the few boxes near it.
    let cell = |v: i32| v.div_euclid(1_000_000);
    let mut by_cell: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (k, b) in boxes.iter().enumerate() {
        for x in cell(b.0)..=cell(b.2) {
            for y in cell(b.1)..=cell(b.3) {
                by_cell.entry((x, y)).or_default().push(k);
            }
        }
    }
    let mut out = vec![Vec::new(); boxes.len()];
    for source in BUILDINGS {
        let path = root
            .join("data/build/buildings")
            .join(format!("{source}-{cc}"))
            .join("points.parquet");
        if !path.exists() {
            continue;
        }
        for_each_batch(&path, |b| {
            let lon = column::<Int32Array>(b, "lon_e7")?;
            let lat = column::<Int32Array>(b, "lat_e7")?;
            for i in 0..b.num_rows() {
                let (x, y) = (lon.value(i), lat.value(i));
                for &k in by_cell.get(&(cell(x), cell(y))).into_iter().flatten() {
                    if let (Some(bx), Some(o)) = (boxes.get(k), out.get_mut(k))
                        && (bx.0..=bx.2).contains(&x)
                        && (bx.1..=bx.3).contains(&y)
                    {
                        o.push((x, y));
                    }
                }
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// Half a row's chip side: a town's is twice a village's.
fn half_of(r: &Row) -> f64 {
    if r.class == "town" {
        2.0 * HALF_M
    } else {
        HALF_M
    }
}

/// A row's chip box in 1e-7°, generous enough for pass 2's shift and the zone's rotation.
fn chip_box(r: &Row) -> (i32, i32, i32, i32) {
    let d = (half_of(r) + 600.0) / 111_000.0;
    let dl = d / r.at.1.to_radians().cos().max(0.1);
    #[allow(clippy::cast_possible_truncation, reason = "degrees as 1e-7°")]
    let e7 = |v: f64| (v * E7).round() as i32;
    (
        e7(r.at.0 - dl),
        e7(r.at.1 - d),
        e7(r.at.0 + dl),
        e7(r.at.1 + d),
    )
}

/// A chip's line of `chips.tsv`.
fn chip_line(r: &Row, png: &str, s: &Scene, frame: &Frame) -> String {
    format!(
        "{}\t{}\t{}\t{png}\t{}\t{}\t{:.2}\t{}\t{:.2}\t{:.2}\t{}\t{}\n",
        r.id,
        r.name,
        r.class,
        s.id,
        s.date,
        s.cloud,
        frame.epsg,
        frame.centre.0,
        frame.centre.1,
        frame.half,
        u8::from(frame.mirror)
    )
}

/// `forge labels chips <cc> [--pass 2] [--only <id,…>]`.
pub(crate) fn chips(root: &Path, cc: &str, args: &[String]) -> Result<String, String> {
    let (mut pass, mut only) = (1_u8, None::<BTreeSet<String>>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--pass", Some(p)) if p == "1" || p == "2" => pass = if p == "2" { 2 } else { 1 },
            ("--only", Some(ids)) => only = Some(ids.split(',').map(str::to_string).collect()),
            _ => return Err("usage: forge labels chips <cc> [--pass 2] [--only <id,…>]".into()),
        }
    }
    let rows: Vec<Row> = rows(root, cc)?
        .into_iter()
        .filter(|r| pass == 1 || r.second)
        .filter(|r| only.as_ref().is_none_or(|o| o.contains(&r.id)))
        .collect();
    let dir = root
        .join("data/build/labels")
        .join(cc)
        .join(format!("pass{pass}"));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let boxes: Vec<(i32, i32, i32, i32)> = rows.iter().map(chip_box).collect();
    let points = buildings(root, cc, &boxes)?;
    let mut tsv = String::from(
        "id\tname\tclass\tpng\tscene\tdate\tcloud\tepsg\tcentre_e\tcentre_n\thalf_m\tmirror\n",
    );
    let (mut drawn, mut missing) = (0_usize, Vec::new());
    for (k, r) in rows.iter().enumerate() {
        let (mirror, shift) = if pass == 2 {
            (true, SHIFT_M)
        } else {
            (false, (0.0, 0.0))
        };
        let b = boxes.get(k).copied().unwrap_or_default();
        let deg = |v: i32| f64::from(v) / E7;
        let found = scenes((deg(b.0), deg(b.1), deg(b.2), deg(b.3)))?;
        let mut done = None;
        for s in found.iter().take(TRIES) {
            if let Some(chip) = draw(s, r.at, half_of(r), mirror, shift)? {
                done = Some((s, chip));
                break;
            }
        }
        let Some((s, (frame, mut rgb))) = done else {
            missing.push(r.id.clone());
            continue;
        };
        let utm = Utm::of_epsg(frame.epsg).ok_or("not UTM")?;
        let pts: Vec<(f64, f64)> = points
            .get(k)
            .into_iter()
            .flatten()
            .map(|&(x, y)| utm.forward((f64::from(x) / E7, f64::from(y) / E7)))
            .collect();
        overlay(&mut rgb, &frame, &pts);
        let name = format!("{:03}-{}.png", k + 1, r.id.replace(':', "-"));
        let file = dir.join(&name);
        fs::write(&file, png(&rgb, SIZE, SIZE)?).map_err(|e| format!("{}: {e}", file.display()))?;
        tsv.push_str(&chip_line(r, &name, s, &frame));
        drawn += 1;
    }
    let file = dir.join("chips.tsv");
    fs::write(&file, tsv).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(format!(
        "labels chips: {drawn} of {} drawn into {} (pass {pass}){}",
        rows.len(),
        dir.display(),
        if missing.is_empty() {
            String::new()
        } else {
            format!("; no clear scene for {}", missing.join(", "))
        }
    ))
}

/// The frames of a pass's chips, by row ID.
fn read_frames(path: &Path) -> Result<BTreeMap<String, Frame>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut frames: BTreeMap<String, Frame> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| {
            f.get(i)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(f64::NAN)
        };
        let (Some(id), Some(epsg)) = (f.first(), f.get(7).and_then(|v| v.parse().ok())) else {
            continue;
        };
        frames.insert(
            (*id).to_string(),
            Frame {
                epsg,
                centre: (num(8), num(9)),
                half: num(10),
                mirror: f.get(11) == Some(&"1"),
            },
        );
    }
    Ok(frames)
}

/// Picks: `id`, `x`, `y`, `notes` per line (`-` for no settlement), each for a chip of `frames`.
fn read_picks(
    picks_path: &str,
    frames: &BTreeMap<String, Frame>,
    pass: u8,
) -> Result<BTreeMap<String, Pick>, String> {
    let text = fs::read_to_string(picks_path).map_err(|e| format!("{picks_path}: {e}"))?;
    let mut picks: BTreeMap<String, Pick> = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') || line.starts_with("id\t") {
            continue;
        }
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let (Some(id), Some(x), Some(y)) = (f.first(), f.get(1), f.get(2)) else {
            return Err(format!("{picks_path}:{}: id, x, y, notes", n + 1));
        };
        let notes = f.get(3).map_or("", |s| s.trim()).to_string();
        let px = match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => Some((x, y)),
            _ if *x == "-" => None,
            _ => return Err(format!("{picks_path}:{}: bad pixel {x},{y}", n + 1)),
        };
        if !frames.contains_key(*id) {
            return Err(format!(
                "{picks_path}:{}: {id} has no chip in pass {pass}",
                n + 1
            ));
        }
        picks.insert((*id).to_string(), (px, notes));
    }
    Ok(picks)
}

/// Writes labels: pass 1 into the sample's own columns, pass 2 into its own file.
fn write_labels(
    root: &Path,
    cc: &str,
    pass: u8,
    labeller: &str,
    labels: BTreeMap<String, (String, String, String)>,
) -> Result<(), String> {
    if pass == 1 {
        let path = sample_path(root, cc);
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut out = String::new();
        for (i, line) in text.lines().enumerate() {
            let mut f = split(line);
            if i > 0
                && let Some((lat, lon, notes)) = f.first().and_then(|id| labels.get(id)).cloned()
            {
                f.resize(11, String::new());
                for (slot, v) in f
                    .iter_mut()
                    .skip(7)
                    .zip([lat, lon, labeller.to_string(), notes])
                {
                    *slot = v;
                }
            }
            let _ = writeln!(
                out,
                "{}",
                f.iter().map(|v| field(v)).collect::<Vec<_>>().join(",")
            );
        }
        fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    } else {
        let path = pass2_path(root, cc);
        let mut all: BTreeMap<String, String> = BTreeMap::new();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines().skip(1) {
                if let Some(id) = split(line).first() {
                    all.insert(id.clone(), line.to_string());
                }
            }
        }
        for (id, (lat, lon, notes)) in labels {
            let line = [id.clone(), lat, lon, labeller.to_string(), notes]
                .iter()
                .map(|v| field(v))
                .collect::<Vec<_>>()
                .join(",");
            all.insert(id, line);
        }
        let mut out = String::from("id,label_lat,label_lon,labeller,notes\n");
        for line in all.values() {
            let _ = writeln!(out, "{line}");
        }
        fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// One pick: the chip's pixel, or none.
type Pick = (Option<(f64, f64)>, String);

/// `forge labels set <cc> <picks.tsv> <labeller> [--pass 2]`.
pub(crate) fn set(root: &Path, cc: &str, args: &[String]) -> Result<String, String> {
    let usage = "usage: forge labels set <cc> <picks.tsv> <labeller> [--pass 2]";
    let (picks_path, labeller, pass) = match args {
        [p, l] => (p, l, 1_u8),
        [p, l, f, two] if f == "--pass" && two == "2" => (p, l, 2_u8),
        _ => return Err(usage.into()),
    };
    let dir = root
        .join("data/build/labels")
        .join(cc)
        .join(format!("pass{pass}"));
    let frames = read_frames(&dir.join("chips.tsv"))?;
    let picks = read_picks(picks_path, &frames, pass)?;
    // The label of each pick, as text: latitude, longitude to 1e-6°.
    let mut labels: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for (id, (px, notes)) in &picks {
        let Some(frame) = frames.get(id) else {
            continue;
        };
        let (lat, lon) = match px {
            Some(p) => {
                let utm = Utm::of_epsg(frame.epsg).ok_or("not UTM")?;
                let (lon, lat) = utm.inverse(frame.at(*p));
                (format!("{lat:.6}"), format!("{lon:.6}"))
            }
            None => (String::new(), String::new()),
        };
        labels.insert(id.clone(), (lat, lon, notes.clone()));
    }
    let n = labels.len();
    write_labels(root, cc, pass, labeller, labels)?;
    Ok(format!(
        "labels set: {n} labels (pass {pass}) by {labeller}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utm_meets_its_reference_points_and_inverts() {
        let z45 = Utm::of_epsg(32645).unwrap();
        // On the central meridian at the equator: the false easting and northing.
        let (e, n) = z45.forward((87.0, 0.0));
        assert!((e - 500_000.0).abs() < 1e-6 && n.abs() < 1e-6, "{e} {n}");
        // The meridian arc to 30° N (Karney's series): 3,320,113.398 m × k0.
        let (_, n) = z45.forward((87.0, 30.0));
        assert!((n - 3_320_113.398 * K0).abs() < 0.01, "{n}");
        // Round trips across a zone, a millimetre or better.
        for (lon, lat) in [(85.324, 27.7172), (80.06, 29.3), (88.2, 26.4), (84.0, 28.0)] {
            let (l2, p2) = z45.inverse(z45.forward((lon, lat)));
            assert!(
                (l2 - lon).abs() < 1e-8 && (p2 - lat).abs() < 1e-8,
                "{lon} {lat}"
            );
        }
        let south = Utm::of_epsg(32733).unwrap();
        let (_, n) = south.forward((15.0, -10.0));
        assert!(n < 10_000_000.0 && n > 8_000_000.0, "{n}");
        assert!(Utm::of_epsg(4326).is_none());
    }

    #[test]
    fn a_frame_maps_pixels_and_back_mirrored_or_not() {
        for mirror in [false, true] {
            let f = Frame {
                epsg: 32645,
                centre: (334_000.0, 3_067_000.0),
                half: HALF_M,
                mirror,
            };
            let (u, v) = f.pixel(f.at((123.0, 456.0)));
            assert!((u - 123.0).abs() < 1e-9 && (v - 456.0).abs() < 1e-9);
            // The chip's middle is its centre.
            let mid = f.at((299.5, 299.5));
            assert!((mid.0 - 334_000.0).abs() < 1e-9 && (mid.1 - 3_067_000.0).abs() < 1e-9);
            // East is to the right, unless mirrored; north is up.
            let (left, right) = (f.at((0.0, 0.0)).0, f.at((599.0, 0.0)).0);
            assert_eq!(left < right, !mirror);
            assert!(f.at((0.0, 0.0)).1 > f.at((0.0, 599.0)).1);
        }
    }

    #[test]
    fn png_is_well_formed() {
        let rgb = vec![200_u8; 4 * 3 * 3];
        let p = png(&rgb, 4, 3).unwrap();
        assert_eq!(p.get(..8).unwrap(), b"\x89PNG\r\n\x1a\n");
        assert_eq!(p.get(12..16).unwrap(), b"IHDR");
        assert_eq!(p.get(16..20).unwrap(), 4_u32.to_be_bytes());
        // IHDR's CRC, computed independently over "IHDR" and its 13 bytes.
        let mut c = flate2::Crc::new();
        c.update(p.get(12..29).unwrap());
        assert_eq!(p.get(29..33).unwrap(), c.sum().to_be_bytes());
        assert!(p.ends_with(b"IEND\xaeB`\x82"));
        // The image data inflates back to filter bytes plus rows.
        let idat = p.windows(4).position(|w| w == b"IDAT").unwrap();
        let len = u32::from_be_bytes(p.get(idat - 4..idat).unwrap().try_into().unwrap()) as usize;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(p.get(idat + 4..idat + 4 + len).unwrap())
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out.len(), 3 * (1 + 4 * 3));
    }

    #[test]
    fn stretch_spreads_a_dull_channel() {
        let mut rgb: Vec<u8> = (0..100_u8)
            .flat_map(|v| [40 + v / 5, 40 + v / 5, 40])
            .collect();
        stretch(&mut rgb);
        let reds: Vec<u8> = rgb.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        assert!(*reds.iter().min().unwrap() < 20 && *reds.iter().max().unwrap() > 235);
    }
}
