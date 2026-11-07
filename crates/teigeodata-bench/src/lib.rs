//! Benchmark workloads, defined once (the [benchmarks](https://github.com/teispace/teigeodata/blob/main/docs/06-engineering/benchmarks.md)
//! method): every harness and, later, every binding runs these same inputs, built from fixed
//! seeds.

use ed25519_dalek::SigningKey;
use teigeodata_core::{E7, Point};
use teigeodata_format::{Encoding, Error, Kind, Writer, signer};

/// `SplitMix64`: a fixed, fast, well-mixed sequence for reproducible inputs.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A sequence from this seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next value.
    pub const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// `n` points uniform over the world on the 1e-7° grid.
#[must_use]
pub fn world_points(n: usize, seed: u64) -> Vec<Point<E7>> {
    let mut r = Rng::new(seed);
    (0..n)
        .filter_map(|_| {
            let v = r.next_u64();
            let lat = i64::try_from(v % 1_800_000_001).ok()? - 900_000_000;
            let lon = i64::try_from((v >> 20) % 3_600_000_001).ok()? - 1_800_000_000;
            Point::from_units(i32::try_from(lat).ok()?, i32::try_from(lon).ok()?)
        })
        .collect()
}

/// The fixed signing key of benchmark packs.
#[must_use]
pub fn bench_key() -> SigningKey {
    SigningKey::from_bytes(&[0x42; 32])
}

/// A signed pack of about `bytes` bytes: three sections of pseudo-random data and a manifest,
/// as a real pack has.
///
/// # Errors
/// From the writer.
pub fn pack_of_size(bytes: usize, seed: u64) -> Result<Vec<u8>, Error> {
    let mut r = Rng::new(seed);
    let sizes = [bytes / 2, bytes / 3, bytes - bytes / 2 - bytes / 3];
    let bodies: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&n| {
            let mut v = Vec::with_capacity(n + 8);
            while v.len() < n {
                v.extend_from_slice(&r.next_u64().to_le_bytes());
            }
            v.truncate(n);
            v
        })
        .collect();
    let mut w = Writer::new("np", "places", "2026.11.0")?;
    w.build_id(seed).created_unix(1_762_473_600);
    let kinds = [Kind::IDMAP, Kind::PLACES, Kind::NAMES];
    let mut all: Vec<(Kind, &[u8])> = kinds
        .into_iter()
        .zip(bodies.iter().map(Vec::as_slice))
        .collect();
    for &(kind, body) in &all {
        let items = u64::try_from(body.len() / 8).unwrap_or(0);
        w.section_bytes(kind, Encoding::RAW, items, body)?;
    }
    // Sections of 1 MiB or more need their block hashes (B12).
    let list = teigeodata_format::blocks::encode(&all)?;
    if let Some(l) = &list {
        let items = teigeodata_format::blocks::item_count(l);
        w.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, items, l)?;
        all.push((Kind::BLOCK_HASHES, l));
    }
    let mut layout = w.layout(true)?;
    let key = bench_key();
    let block = signer(&key)(layout.digest());
    layout.set_signature(&block)?;
    let refs: Vec<&[u8]> = layout
        .placed()
        .iter()
        .filter_map(|p| all.iter().find(|s| s.0 == p.kind).map(|s| s.1))
        .collect();
    layout.assemble(&refs)
}

/// The integer columns of B1, from a real dataset, rows in Hilbert order.
#[derive(Clone, Debug, Default)]
pub struct Columns {
    /// Latitude on the 1e-6° grid, biased to u32 (order-preserving).
    pub lat: Vec<u32>,
    /// Longitude on the 1e-6° grid, biased to u32.
    pub lon: Vec<u32>,
    /// Elevation in metres, biased to u32.
    pub elevation: Vec<u32>,
    /// Source IDs, sorted ascending (the shape of an ID map).
    pub ids: Vec<u32>,
}

/// An i32 as an order-preserving u32: `i32::MIN` → 0.
#[must_use]
pub const fn bias(v: i32) -> u32 {
    v.cast_unsigned() ^ 0x8000_0000
}

/// The `GeoNames` Nepal extract (S1's input), if present at `path`: one row per place, in Hilbert
/// order of the point. `None` if the file is missing, so benchmarks can skip it.
#[must_use]
pub fn geonames_columns(path: &std::path::Path) -> Option<Columns> {
    use teigeodata_core::{E6, hilbert};
    let text = std::fs::read_to_string(path).ok()?;
    let mut rows: Vec<(u64, u32, i32, i32, i32)> = text
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let id = f.first()?.parse().ok()?;
            let lat: f64 = f.get(4)?.parse().ok()?;
            let lon: f64 = f.get(5)?.parse().ok()?;
            let dem = f.get(16).and_then(|v| v.parse().ok()).unwrap_or(0);
            let p = Point::<E6>::from_degrees(lat, lon).ok()?;
            Some((hilbert(p), id, p.lat(), p.lon(), dem))
        })
        .collect();
    rows.sort_unstable();
    let mut ids: Vec<u32> = rows.iter().map(|r| r.1).collect();
    ids.sort_unstable();
    Some(Columns {
        lat: rows.iter().map(|r| bias(r.2)).collect(),
        lon: rows.iter().map(|r| bias(r.3)).collect(),
        elevation: rows.iter().map(|r| bias(r.4)).collect(),
        ids,
    })
}

/// PLACES and HIERARCHY input from a real dataset: one row per place in Hilbert order, with
/// kinds, admin levels and parents by row.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// Points, Hilbert-sorted.
    pub points: Vec<Point<teigeodata_core::E6>>,
    /// Kinds.
    pub kinds: Vec<teigeodata_core::PlaceKind>,
    /// Admin levels (0 for non-admin places).
    pub levels: Vec<u8>,
    /// Parent row of each row.
    pub parents: Vec<Option<u32>>,
}

/// Hilbert index, point, kind, admin level, (admin1, admin2) codes.
type GeonamesRow = (
    u64,
    Point<teigeodata_core::E6>,
    teigeodata_core::PlaceKind,
    u8,
    [String; 2],
);

/// A tree from the `GeoNames` Nepal extract, if present: the country (`PCLI`) at the root, each
/// `ADMn` unit under the unit of the level above with the same codes, and every other place
/// under its most specific unit. `GeoNames` codes stand in for point-in-polygon here, so the
/// shape (how many children, how deep) is real enough for sizes and speeds, not for accuracy.
#[must_use]
pub fn geonames_tree(path: &std::path::Path) -> Option<Tree> {
    use std::collections::HashMap;
    use teigeodata_core::{E6, PlaceKind, hilbert};
    let text = std::fs::read_to_string(path).ok()?;
    let mut rows: Vec<GeonamesRow> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (Some(lat), Some(lon)) = (f.get(4), f.get(5)) else {
            continue;
        };
        let Ok(p) = Point::<E6>::from_degrees(lat.parse().ok()?, lon.parse().ok()?) else {
            continue;
        };
        let (class, code) = (
            f.get(6).copied().unwrap_or(""),
            f.get(7).copied().unwrap_or(""),
        );
        let (kind, level) = match (class, code) {
            ("A", "PCLI") => (PlaceKind::Country, 0),
            ("A", c) if c.starts_with("ADM") => {
                let level = c.get(3..4).and_then(|d| d.parse().ok()).unwrap_or(0);
                if level == 0 {
                    (PlaceKind::Locality, 0)
                } else {
                    (PlaceKind::Admin, level)
                }
            }
            ("P", _) => (PlaceKind::Settlement, 0),
            ("H" | "T" | "U" | "V", _) => (PlaceKind::Natural, 0),
            _ => (PlaceKind::Locality, 0),
        };
        let codes = [f.get(10), f.get(11)].map(|c| c.copied().unwrap_or("").to_owned());
        rows.push((hilbert(p), p, kind, level, codes));
    }
    rows.sort_by_key(|r| (r.0, r.1.lat(), r.1.lon()));
    // The unit for each code prefix: level 1 by admin1, level 2 by (admin1, admin2).
    let mut country = None;
    let mut units: HashMap<(u8, String, String), u32> = HashMap::new();
    for (row, r) in rows.iter().enumerate() {
        let row = u32::try_from(row).ok()?;
        match (r.2, r.3) {
            (PlaceKind::Country, _) => country = country.or(Some(row)),
            (PlaceKind::Admin, 1) => {
                units
                    .entry((1, r.4[0].clone(), String::new()))
                    .or_insert(row);
            }
            (PlaceKind::Admin, 2) => {
                units
                    .entry((2, r.4[0].clone(), r.4[1].clone()))
                    .or_insert(row);
            }
            _ => {}
        }
    }
    let unit = |level: u8, codes: &[String; 2]| {
        let key = |l: u8| {
            (
                l,
                codes[0].clone(),
                if l == 2 {
                    codes[1].clone()
                } else {
                    String::new()
                },
            )
        };
        (1..=level.min(2))
            .rev()
            .find_map(|l| units.get(&key(l)).copied())
    };
    let parents = rows
        .iter()
        .enumerate()
        .map(|(row, r)| {
            let up = match (r.2, r.3) {
                (PlaceKind::Country, _) => None,
                (PlaceKind::Admin, l) => unit(l.saturating_sub(1), &r.4).or(country),
                _ => unit(2, &r.4).or(country),
            };
            up.filter(|&p| usize::try_from(p).ok() != Some(row))
        })
        .collect();
    Some(Tree {
        points: rows.iter().map(|r| r.1).collect(),
        kinds: rows.iter().map(|r| r.2).collect(),
        levels: rows.iter().map(|r| r.3).collect(),
        parents,
    })
}

/// The default location of the S1 input in a checkout: `data/cache/geonames-np/NP.txt`.
#[must_use]
pub fn geonames_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cache/geonames-np/NP.txt")
}

/// LEB128 zigzag deltas: S1's baseline (no random access).
#[must_use]
pub fn leb128_delta(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 2);
    let mut prev = 0_u32;
    for &v in values {
        let d = v.wrapping_sub(prev).cast_signed();
        prev = v;
        let mut z = ((d << 1) ^ (d >> 31)).cast_unsigned();
        while z >= 0x80 {
            out.push(u8::try_from(z & 0x7f).unwrap_or(0) | 0x80);
            z >>= 7;
        }
        out.push(u8::try_from(z).unwrap_or(0));
    }
    out
}

/// Decodes [`leb128_delta`] into `out`; returns the number of values.
pub fn leb128_delta_decode(bytes: &[u8], out: &mut [u32]) -> usize {
    let (mut n, mut prev, mut z, mut shift) = (0, 0_u32, 0_u32, 0);
    for &b in bytes {
        z |= u32::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            let d = (z >> 1) ^ (z & 1).wrapping_neg();
            prev = prev.wrapping_add(d);
            if let Some(o) = out.get_mut(n) {
                *o = prev;
            }
            n += 1;
            z = 0;
            shift = 0;
        } else {
            shift = (shift + 7) & 31;
        }
    }
    n
}

/// The classic iterative Hilbert loop: the definition of the order and the A/B baseline for
/// `teigeodata_core::hilbert`.
#[must_use]
#[allow(
    clippy::many_single_char_names,
    reason = "the textbook algorithm's names"
)]
pub fn hilbert_reference(p: Point<E7>) -> u64 {
    const N: u64 = 1 << 32;
    let mut x = u64::try_from(i64::from(p.lon()) + 1_800_000_000).unwrap_or(0);
    let mut y = u64::try_from(i64::from(p.lat()) + 900_000_000).unwrap_or(0);
    let mut d = 0_u64;
    let mut s = N >> 1;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = N - 1 - x;
                y = N - 1 - y;
            }
            core::mem::swap(&mut x, &mut y);
        }
        s >>= 1;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::{hilbert_reference, pack_of_size, world_points};
    use teigeodata_format::Pack;

    #[test]
    fn leb128_round_trips() {
        let v: Vec<u32> = (0..5000_u32)
            .map(|i| i.wrapping_mul(2_654_435_761))
            .collect();
        let mut out = vec![0; v.len()];
        assert_eq!(
            super::leb128_delta_decode(&super::leb128_delta(&v), &mut out),
            v.len()
        );
        assert_eq!(out, v);
    }

    #[test]
    fn workloads_are_valid() {
        let points = world_points(10_000, 1);
        assert_eq!(points.len(), 10_000);
        assert_eq!(points, world_points(10_000, 1));
        for p in &points {
            assert_eq!(teigeodata_core::hilbert(*p), hilbert_reference(*p));
        }
        let pack = pack_of_size(100_000, 1);
        assert!(pack.as_ref().is_ok_and(|b| Pack::open_verified(b).is_ok()));
        assert_eq!(pack, pack_of_size(100_000, 1));
        // Large enough to need block hashes: it still opens, verifies, and its blocks prove.
        let big = pack_of_size(6 << 20, 1);
        let open = big.as_ref().map(|b| Pack::open_verified(b));
        assert!(open.as_ref().is_ok_and(Result::is_ok));
        if let Ok(Ok(p)) = &open {
            assert!(
                p.blocks(teigeodata_format::Kind::IDMAP)
                    .is_ok_and(|b| b.is_some())
            );
        }
    }
}
