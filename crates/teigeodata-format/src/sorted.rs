//! A strictly increasing list of u64 keys stored as two packed columns (high and low 32 bits),
//! or of u32 keys as one: the one search IDMAP, REDIRECTS and COMPONENTS' sparse presence share
//! (B5, B6).
//!
//! A lookup binary-searches the blocks by their first key, which [`Packed::block_first`] reads
//! in O(1), then unpacks one block of each column and searches it.

use teigeodata_core::bitpack::{BLOCK_LEN, Packed};

/// The key of `(hi, lo)`.
pub(crate) const fn key(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

/// High and low halves of a sorted key list, of equal length; no high column means the high
/// halves are all 0 (u32 keys).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sorted<'a> {
    pub(crate) hi: Option<Packed<'a>>,
    pub(crate) lo: Packed<'a>,
}

impl Sorted<'_> {
    fn block_key(&self, b: usize) -> Option<u64> {
        let hi = match self.hi {
            Some(hi) => hi.block_first(b)?,
            None => 0,
        };
        Some(key(hi, self.lo.block_first(b)?))
    }

    /// The first and the last key, `None` when empty: O(1), two direct reads.
    pub(crate) fn bounds(&self) -> Option<(u64, u64)> {
        let last = self.lo.len().checked_sub(1)?;
        let at = |i: usize| {
            let hi = match self.hi {
                Some(hi) => hi.get(i)?,
                None => 0,
            };
            Some(key(hi, self.lo.get(i)?))
        };
        Some((at(0)?, at(last)?))
    }

    /// The index of `k`, if present.
    pub(crate) fn find(&self, k: u64) -> Option<usize> {
        let b = self.block_of(k)?;
        let (mut his, mut los) = ([0_u32; BLOCK_LEN], [0_u32; BLOCK_LEN]);
        let n = self.lo.decode_block(b, &mut los);
        // The high halves are almost always one value per block: no unpacking then.
        let hi_constant = match self.hi {
            Some(hi) => hi.block_constant(b).or_else(|| {
                hi.decode_block(b, &mut his);
                None
            }),
            None => Some(0),
        };
        let at = |j: usize| {
            let hi = hi_constant.or_else(|| his.get(j).copied()).unwrap_or(0);
            key(hi, los.get(j).copied().unwrap_or(0))
        };
        let (mut low, mut high) = (0, n);
        while low < high {
            let mid = low + (high - low) / 2;
            match at(mid).cmp(&k) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return Some(b * BLOCK_LEN + mid),
            }
        }
        None
    }

    /// The last block whose first key is at most `k`. Interpolation first: dense serials land on
    /// the right block in one or two probes. Each probe and its neighbour narrow the window, so
    /// after a few rounds the binary search that follows has little left, and clustered keys
    /// cost at most those rounds more than a binary search alone.
    fn block_of(&self, k: u64) -> Option<usize> {
        let blocks = self.lo.blocks();
        if self.block_key(0)? > k {
            return None;
        }
        // Invariant: first(low) <= k, and first(high) > k or high == blocks.
        let (mut low, mut high) = (0, blocks);
        for _ in 0..3 {
            if high - low <= 2 {
                break;
            }
            let (kl, kh) = (self.block_key(low)?, self.block_key(high - 1)?);
            if k >= kh {
                return Some(high - 1);
            }
            let span = u128::from(kh - kl).max(1);
            let off = u128::from(k - kl) * (high - 1 - low) as u128 / span;
            let g = (low + usize::try_from(off).unwrap_or(0)).clamp(low + 1, high - 2);
            if self.block_key(g)? <= k {
                if self.block_key(g + 1)? > k {
                    return Some(g);
                }
                low = g + 1;
            } else {
                high = g;
            }
        }
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if self.block_key(mid)? <= k {
                low = mid;
            } else {
                high = mid;
            }
        }
        Some(low)
    }

    /// Every key, in order, streamed a block at a time.
    pub(crate) fn iter(&self) -> impl Iterator<Item = u64> + '_ {
        let n = self.lo.len();
        let mut hi = self.hi.map(|h| h.values(0..n));
        self.lo
            .values(0..n)
            .map(move |l| key(hi.as_mut().and_then(Iterator::next).unwrap_or(0), l))
    }

    /// Whether the keys are strictly increasing.
    pub(crate) fn is_strictly_increasing(&self) -> bool {
        let mut prev = None;
        self.iter().all(|k| {
            let ok = prev < Some(k);
            prev = Some(k);
            ok
        })
    }
}

/// The high and low halves of each key (feature `write`).
#[cfg(feature = "write")]
pub(crate) fn split(
    keys: impl Iterator<Item = u64>,
) -> (alloc::vec::Vec<u32>, alloc::vec::Vec<u32>) {
    #[allow(clippy::cast_possible_truncation, reason = "the halves of a u64")]
    keys.map(|k| ((k >> 32) as u32, k as u32)).unzip()
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "test inputs are cut from a 64-bit generator on purpose"
)]
mod tests {
    use super::{Sorted, key, split};
    use std::vec::Vec;
    use teigeodata_core::bitpack::{Packed, encode_auto};

    /// Every key and its neighbours, against the sorted model, for key sets that send the
    /// search down each path: dense (interpolation exact), clustered and exponential (the
    /// neighbour checks and the binary fallback), many high halves, and tiny lists.
    #[test]
    fn find_agrees_with_the_model() {
        let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        // Under Miri the large sets keep their shape at a fiftieth of the size, and the small
        // lists keep one per block edge (15 minutes at a twentieth and every edge, 2026-11-07).
        let size = |n: u64| if cfg!(miri) { n / 50 } else { n };
        let mut sets: Vec<Vec<u64>> = Vec::new();
        sets.push((1..size(5000)).collect());
        let half = size(1500);
        sets.push(
            (0..2 * half)
                .map(|i| if i < half { i } else { (1 << 40) + i })
                .collect(),
        );
        sets.push((0..40).map(|i| 1_u64 << i).collect());
        sets.push((0..size(4000)).map(|_| next() % 100_000).collect());
        sets.push((0..size(2000)).map(|_| next()).collect());
        sets.push(
            (0..size(2000))
                .map(|i| (i % 7) << 32 | (next() % 50_000))
                .collect(),
        );
        let small: &[u64] = if cfg!(miri) {
            &[1, 128, 129]
        } else {
            &[1, 2, 127, 128, 129, 255, 256, 257, 385]
        };
        for &n in small {
            sets.push((0..n).map(|i| 10 + 3 * i).collect());
        }
        for mut keys in sets {
            keys.sort_unstable();
            keys.dedup();
            let (hi, lo) = split(keys.iter().copied());
            let (hb, lb) = (encode_auto(&hi), encode_auto(&lo));
            let (Ok(hi), Ok(lo)) = (Packed::parse(&hb), Packed::parse(&lb)) else {
                panic!("encode");
            };
            // u32 keys also as one column, with no high half.
            let narrow = keys.iter().all(|&k| k >> 32 == 0);
            let views = [
                Some(Sorted { hi: Some(hi), lo }),
                narrow.then_some(Sorted { hi: None, lo }),
            ];
            for sorted in views.into_iter().flatten() {
                assert!(sorted.is_strictly_increasing());
                assert!(sorted.iter().eq(keys.iter().copied()));
                for (i, &k) in keys.iter().enumerate() {
                    assert_eq!(sorted.find(k), Some(i), "key {k}");
                    for probe in [k.wrapping_sub(1), k.wrapping_add(1)] {
                        assert_eq!(
                            sorted.find(probe),
                            keys.binary_search(&probe).ok(),
                            "probe {probe}"
                        );
                    }
                }
                assert_eq!(sorted.find(u64::MAX), keys.binary_search(&u64::MAX).ok());
                assert_eq!(sorted.find(0), keys.binary_search(&0).ok());
            }
        }
        assert_eq!(key(1, 2), (1 << 32) | 2);
    }
}
