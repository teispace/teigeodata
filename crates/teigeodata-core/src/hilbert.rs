//! The Hilbert index: the row order of every pack (spike S1: 22% smaller coordinate columns than
//! source order, 7% smaller than Morton).
//!
//! The curve covers a 2³² × 2³² square: `x` is the longitude and `y` the latitude on the 1e-7°
//! grid, shifted to be non-negative. Points on the 1e-6° grid are converted exactly first, so a
//! point has the same index whichever grid stores it.
//!
//! **The order is part of the format.** It is defined by the classic iterative algorithm
//! (`reference` in the tests). The function here is a branch-free parallel-prefix version
//! (after rawrunprotected's `hilbert_curves`, widened from 16 to 32 bits per axis); a
//! differential test proves the two equal.

use crate::grid::{Grid, Point};

/// The Hilbert index of a point. Equal points have equal indexes on every grid.
///
/// ```
/// use teigeodata_core::{E6, E7, Point, hilbert};
/// let p = Point::<E6>::from_degrees(27.7172, 85.324)?;
/// assert_eq!(hilbert(p), hilbert(p.to_e7()));
/// # Ok::<(), teigeodata_core::GridError>(())
/// ```
#[must_use]
pub fn hilbert<G: Grid>(p: Point<G>) -> u64 {
    let p = p.to_e7();
    // The range checks of `Point` keep both values in 0..=3.6e9, below 2^32.
    let x = p.lon().wrapping_add(1_800_000_000).cast_unsigned();
    let y = p.lat().wrapping_add(900_000_000).cast_unsigned();
    xy_to_index(x, y)
}

/// The Hilbert index of `(x, y)` on the 2³² × 2³² curve.
#[must_use]
#[allow(
    clippy::many_single_char_names,
    reason = "the published algorithm's names"
)]
pub(crate) fn xy_to_index(x: u32, y: u32) -> u64 {
    const M: u64 = 0xffff_ffff;
    let (x, y) = (u64::from(x), u64::from(y));

    // First round of the prefix scan, primed with x and y.
    let a = x ^ y;
    let b = M ^ a;
    let c = M ^ (x | y);
    let d = x & (y ^ M);
    let mut aa = a | (b >> 1);
    let mut bb = (a >> 1) ^ a;
    let mut cc = ((c >> 1) ^ (b & (d >> 1))) ^ c;
    let mut dd = ((a & (c >> 1)) ^ (d >> 1)) ^ d;

    // Rounds at distance 2, 4 and 8.
    for shift in [2, 4, 8] {
        let (a, b, c, d) = (aa, bb, cc, dd);
        aa = (a & (a >> shift)) ^ (b & (b >> shift));
        bb = (a & (b >> shift)) ^ (b & ((a ^ b) >> shift));
        cc ^= (a & (c >> shift)) ^ (b & (d >> shift));
        dd ^= (b & (c >> shift)) ^ ((a ^ b) & (d >> shift));
    }
    // Final round at distance 16: only c and d are needed.
    {
        let (a, b, c, d) = (aa, bb, cc, dd);
        cc ^= (a & (c >> 16)) ^ (b & (d >> 16));
        dd ^= (b & (c >> 16)) ^ ((a ^ b) & (d >> 16));
    }

    // Undo the prefix scan and recover the index bits.
    let a = cc ^ (cc >> 1);
    let b = dd ^ (dd >> 1);
    let i0 = x ^ y;
    let i1 = b | (M ^ (i0 | a));
    (spread(i1 & M) << 1) | spread(i0 & M)
}

/// Spreads the low 32 bits of `v` to the even bits of a u64.
const fn spread(mut v: u64) -> u64 {
    v = (v | (v << 16)) & 0x0000_ffff_0000_ffff;
    v = (v | (v << 8)) & 0x00ff_00ff_00ff_00ff;
    v = (v | (v << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
    v = (v | (v << 2)) & 0x3333_3333_3333_3333;
    (v | (v << 1)) & 0x5555_5555_5555_5555
}

#[cfg(test)]
mod tests {
    use super::{hilbert, xy_to_index};
    use crate::grid::{E6, E7, Point};

    const N: u64 = 1 << 32;

    /// The definition: the classic iterative xy → d on the 2³² × 2³² square.
    fn reference(x: u32, y: u32) -> u64 {
        let (mut x, mut y) = (u64::from(x), u64::from(y));
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

    /// The inverse of the reference: d → (x, y).
    #[allow(
        clippy::many_single_char_names,
        reason = "the textbook algorithm's names"
    )]
    fn inverse(d: u64) -> (u64, u64) {
        let (mut x, mut y) = (0_u64, 0_u64);
        let mut t = d;
        let mut s = 1_u64;
        while s < N {
            let rx = 1 & (t / 2);
            let ry = 1 & (t ^ rx);
            if ry == 0 {
                if rx == 1 {
                    x = s - 1 - x;
                    y = s - 1 - y;
                }
                core::mem::swap(&mut x, &mut y);
            }
            x += s * rx;
            y += s * ry;
            t /= 4;
            s *= 2;
        }
        (x, y)
    }

    fn rng(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    #[test]
    fn equals_the_reference() {
        let edges = [0, 1, 2, 3, 0x7fff_ffff, 0x8000_0000, 0xffff_fffe, u32::MAX];
        for &x in &edges {
            for &y in &edges {
                assert_eq!(xy_to_index(x, y), reference(x, y), "{x} {y}");
            }
        }
        // Every cell of the first 64 × 64 block, where each sub-curve turns.
        for x in 0..64 {
            for y in 0..64 {
                assert_eq!(xy_to_index(x, y), reference(x, y), "{x} {y}");
            }
        }
        let mut s = 0x2545_f491_4f6c_dd1d;
        for _ in 0..1_000_000 {
            let v = rng(&mut s);
            let [x, y] = [v >> 32, v & 0xffff_ffff].map(|h| u32::try_from(h).unwrap_or_default());
            assert_eq!(xy_to_index(x, y), reference(x, y), "{x} {y}");
        }
    }

    #[test]
    fn is_a_continuous_bijection() {
        // The inverse undoes it, and consecutive indexes are neighbouring cells: the property
        // that makes Hilbert order compress (S1).
        let mut s = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..20_000 {
            let start = rng(&mut s).wrapping_sub(64);
            let mut prev = inverse(start);
            for d in start.wrapping_add(1)..start.saturating_add(64) {
                let (x, y) = inverse(d);
                assert_eq!(prev.0.abs_diff(x) + prev.1.abs_diff(y), 1, "d = {d}");
                let (xu, yu) = (u32::try_from(x), u32::try_from(y));
                if let (Ok(xu), Ok(yu)) = (xu, yu) {
                    assert_eq!(xy_to_index(xu, yu), d);
                }
                prev = (x, y);
            }
        }
    }

    #[test]
    fn same_index_on_both_grids() {
        let p = Point::<E6>::from_units(27_717_200, 85_324_000).unwrap_or_else(|| unreachable!());
        assert_eq!(hilbert(p), hilbert(p.to_e7()));
        let corners = [(-90, -180), (-90, 180), (90, -180), (90, 180), (0, 0)];
        for (lat, lon) in corners {
            let q = Point::<E7>::from_units(lat * 10_000_000, lon * 10_000_000);
            assert!(q.is_some());
            if let Some(q) = q {
                let x = u32::try_from(i64::from(q.lon()) + 1_800_000_000).unwrap_or_default();
                let y = u32::try_from(i64::from(q.lat()) + 900_000_000).unwrap_or_default();
                assert_eq!(hilbert(q), reference(x, y));
            }
        }
    }
}
