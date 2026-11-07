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
