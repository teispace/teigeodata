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
    for (kind, body) in [Kind::IDMAP, Kind::PLACES, Kind::NAMES]
        .into_iter()
        .zip(&bodies)
    {
        let items = u64::try_from(body.len() / 8).unwrap_or(0);
        w.section_bytes(kind, Encoding::RAW, items, body)?;
    }
    let mut layout = w.layout(true)?;
    let key = bench_key();
    let block = signer(&key)(layout.digest());
    layout.set_signature(&block)?;
    let refs: Vec<&[u8]> = bodies.iter().map(Vec::as_slice).collect();
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
    }
}
