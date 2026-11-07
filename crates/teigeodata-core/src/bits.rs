//! A bitmap with a rank directory (B6, D-076): which rows have a value, and the index of a row's
//! value among the present ones. An optional field stores a bitmap and a dense column of the
//! present values only, so absent data costs about one bit per row.
//!
//! The directory follows poppy (Zhou, Andersen and Kaminsky, SEA 2013): one u64 per block of
//! 2048 bits, holding the number of ones before the block (low 32 bits) and the ones in each of
//! its first three 512-bit sub-blocks (10 bits each, from bit 32). Overhead 3.125%. A rank reads
//! one entry, adds at most three counts and popcounts at most eight words.
//!
//! Layout, little-endian:
//! ```text
//! len u32 (bits), reserved u32 (0)
//! words     [u64; ceil(len / 64)]     bit i is bit i % 64 of word i / 64; bits past len are 0
//! directory [u64; ceil(len / 2048)]
//! ```
//! [`Bits::parse`] checks the structure in O(1); [`Bits::check`] proves the directory in O(len).

use core::fmt;

const HEADER: usize = 8;
const BLOCK: usize = 2048;
const SUB: usize = 512;

/// Why bytes are not a bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BitsError {
    /// Not as long as the header says.
    Length,
    /// Reserved bytes are not zero, or bits past the end are set.
    Header,
    /// The directory does not count the bits ([`Bits::check`]).
    Directory,
}

impl fmt::Display for BitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "the bitmap is not as long as its header says",
            Self::Header => "the bitmap header or its padding bits are invalid",
            Self::Directory => "the bitmap rank directory is wrong",
        })
    }
}

impl core::error::Error for BitsError {}

/// A validated bitmap, borrowing its bytes.
#[derive(Clone, Copy, Debug)]
pub struct Bits<'a> {
    len: usize,
    words: &'a [[u8; 8]],
    directory: &'a [[u8; 8]],
}

fn word(w: [u8; 8]) -> u64 {
    u64::from_le_bytes(w)
}

/// The ones of `words`.
fn ones(words: &[[u8; 8]]) -> u32 {
    words.iter().map(|&w| word(w).count_ones()).sum()
}

impl<'a> Bits<'a> {
    /// Checks the structure: lengths, reserved bytes, and that no bit past the end is set.
    ///
    /// # Errors
    /// [`BitsError::Length`] or [`BitsError::Header`].
    pub fn parse(bytes: &'a [u8]) -> Result<Self, BitsError> {
        let (&[l0, l1, l2, l3, r0, r1, r2, r3], rest) = bytes
            .split_first_chunk::<HEADER>()
            .ok_or(BitsError::Length)?;
        if [r0, r1, r2, r3] != [0; 4] {
            return Err(BitsError::Header);
        }
        let len =
            usize::try_from(u32::from_le_bytes([l0, l1, l2, l3])).map_err(|_| BitsError::Length)?;
        let (n_words, n_dir) = (len.div_ceil(64), len.div_ceil(BLOCK));
        let (all, tail) = rest.as_chunks::<8>();
        if !tail.is_empty() || all.len() != n_words + n_dir {
            return Err(BitsError::Length);
        }
        let (words, directory) = all.split_at(n_words);
        let used = len % 64;
        if used != 0 && words.last().is_some_and(|&w| word(w) >> used != 0) {
            return Err(BitsError::Header);
        }
        Ok(Self {
            len,
            words,
            directory,
        })
    }

    /// The number of bits.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether there are no bits.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bit `i`; `false` past the end.
    #[must_use]
    pub fn get(&self, i: usize) -> bool {
        self.words
            .get(i / 64)
            .is_some_and(|&w| word(w) >> (i % 64) & 1 == 1)
    }

    /// The number of ones before bit `i` (all of them for `i ≥ len`).
    #[must_use]
    pub fn rank(&self, i: usize) -> usize {
        let i = i.min(self.len);
        // At `len` on a block boundary there is no entry past the last block: count from that
        // block's last sub-block instead.
        let (block, sub) = match (i / BLOCK, i % BLOCK / SUB) {
            (b, s) if b < self.directory.len() => (b, s),
            _ if self.directory.is_empty() => return 0,
            _ => (self.directory.len() - 1, 3),
        };
        let entry = self.directory.get(block).map_or(0, |&e| word(e));
        let mut r = entry & 0xffff_ffff;
        for k in 0..sub {
            r += entry >> (32 + 10 * k) & 0x3ff;
        }
        let start = block * (BLOCK / 64) + sub * (SUB / 64);
        let end = i / 64;
        r += u64::from(ones(self.words.get(start..end).unwrap_or_default()));
        if !i.is_multiple_of(64)
            && let Some(&w) = self.words.get(end)
        {
            r += u64::from((word(w) & ((1 << (i % 64)) - 1)).count_ones());
        }
        usize::try_from(r).unwrap_or(usize::MAX)
    }

    /// The number of ones.
    #[must_use]
    pub fn count(&self) -> usize {
        self.rank(self.len)
    }

    /// If bit `i` is set, its index among the ones: the position of row `i`'s value in the
    /// dense column of present values.
    #[must_use]
    pub fn index(&self, i: usize) -> Option<usize> {
        self.get(i).then(|| self.rank(i))
    }

    /// The set bits, ascending, a word at a time.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(k, &w)| {
            let mut w = word(w);
            core::iter::from_fn(move || {
                (w != 0).then(|| {
                    let b = w.trailing_zeros() as usize;
                    w &= w - 1;
                    k * 64 + b
                })
            })
        })
    }

    /// Proves the directory against the bits. O(len).
    ///
    /// # Errors
    /// [`BitsError::Directory`] if any entry is wrong.
    pub fn check(&self) -> Result<(), BitsError> {
        let mut before = 0_u64;
        for (&e, block) in self.directory.iter().zip(self.words.chunks(BLOCK / 64)) {
            if word(e) != entry(before, block) {
                return Err(BitsError::Directory);
            }
            before += u64::from(ones(block));
        }
        Ok(())
    }
}

/// The directory entry of a block whose words start at `words`, with `before` ones before it.
fn entry(before: u64, words: &[[u8; 8]]) -> u64 {
    let mut e = before & 0xffff_ffff;
    for k in 0..3 {
        let sub = words.get(k * 8..).unwrap_or_default();
        e |= u64::from(ones(sub.get(..8).unwrap_or(sub))) << (32 + 10 * k);
    }
    e
}

/// Encodes a bitmap of `len` bits with the given ones (feature `alloc`).
///
/// # Errors
/// [`BitsError::Length`] if a one is at or past `len`, or `len` is above `u32::MAX`.
#[cfg(feature = "alloc")]
pub fn encode(
    len: usize,
    ones_at: impl IntoIterator<Item = usize>,
) -> Result<alloc::vec::Vec<u8>, BitsError> {
    let len32 = u32::try_from(len).map_err(|_| BitsError::Length)?;
    let mut words = alloc::vec![0_u64; len.div_ceil(64)];
    for i in ones_at {
        let w = words
            .get_mut(i / 64)
            .filter(|_| i < len)
            .ok_or(BitsError::Length)?;
        *w |= 1 << (i % 64);
    }
    let bytes: alloc::vec::Vec<[u8; 8]> = words.iter().map(|w| w.to_le_bytes()).collect();
    let mut out = alloc::vec::Vec::with_capacity(HEADER + 8 * (words.len() + len.div_ceil(BLOCK)));
    out.extend_from_slice(&len32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    for w in &bytes {
        out.extend_from_slice(w);
    }
    let mut before = 0_u64;
    for block in bytes.chunks(BLOCK / 64) {
        out.extend_from_slice(&entry(before, block).to_le_bytes());
        before += u64::from(ones(block));
    }
    Ok(out)
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "test inputs are cut from a 64-bit generator on purpose"
)]
mod tests {
    use super::{Bits, BitsError, encode};
    use std::vec::Vec;

    fn rng(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "exhaustive sizes: hours under Miri; smaller tests here and libFuzzer cover it"
    )]
    fn rank_agrees_with_counting() {
        let mut s = 0x1234_5678_9abc_def1;
        for len in [
            0, 1, 63, 64, 65, 511, 512, 513, 2047, 2048, 2049, 6000, 20_000,
        ] {
            for per_mille in [0, 10, 100, 500, 900, 1000] {
                let set: Vec<bool> = (0..len).map(|_| rng(&mut s) % 1000 < per_mille).collect();
                let bytes = encode(len, (0..len).filter(|&i| set[i])).unwrap_or_default();
                let b = Bits::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(b.check(), Ok(()));
                assert_eq!(b.len(), len);
                let mut r = 0;
                for (i, &bit) in set.iter().enumerate() {
                    assert_eq!(b.rank(i), r, "len {len} density {per_mille} rank {i}");
                    assert_eq!(b.get(i), bit);
                    assert_eq!(b.index(i), bit.then_some(r));
                    r += usize::from(bit);
                }
                assert_eq!(b.rank(len), r);
                assert_eq!(b.rank(len + 100), r);
                assert_eq!(b.count(), r);
                assert!(!b.get(len));
                assert_eq!(
                    // Bounded: an iterator that never ends fails here instead of eating memory.
                    b.iter().take(len + 1).collect::<Vec<_>>(),
                    (0..len).filter(|&i| set[i]).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn rejects() {
        let good = encode(5000, (0..5000).step_by(3)).unwrap_or_default();
        assert!(Bits::parse(&good).is_ok());
        assert_eq!(Bits::parse(&good[..7]).err(), Some(BitsError::Length));
        assert_eq!(
            Bits::parse(&good[..good.len() - 8]).err(),
            Some(BitsError::Length)
        );
        let mut longer = good.clone();
        longer.extend_from_slice(&[0; 8]);
        assert_eq!(Bits::parse(&longer).err(), Some(BitsError::Length));
        let mut b = good.clone();
        b[5] = 1;
        assert_eq!(Bits::parse(&b).err(), Some(BitsError::Header));
        // The first bit past the end: 5000 bits end in word 78 at bit 8 (byte 1, bit 0), and
        // the last one.
        for (byte, bit) in [(1, 0x01), (7, 0x80)] {
            let mut b = good.clone();
            b[8 + 78 * 8 + byte] |= bit;
            assert_eq!(Bits::parse(&b).err(), Some(BitsError::Header));
        }
        // A wrong directory entry parses, and the check finds it: in the cumulative count
        // (entry byte 3) and in each sub-block count (bits 32, 42 and 52: bytes 4, 5 and 6).
        let directory = good.len() - 8 * 3;
        for (entry, byte, bit) in [(2, 3, 0x01), (0, 4, 0x01), (1, 5, 0x04), (2, 6, 0x10)] {
            let mut b = good.clone();
            b[directory + 8 * entry + byte] ^= bit;
            let bits = Bits::parse(&b).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(bits.check(), Err(BitsError::Directory), "{entry} {byte}");
        }
        assert_eq!(encode(10, [10]).err(), Some(BitsError::Length));
    }

    #[test]
    fn never_panics() {
        let base = encode(3000, (0..3000).filter(|i| i % 7 < 3)).unwrap_or_default();
        let mut s = 99;
        for _ in 0..crate::cases(20_000) {
            let mut b = base.clone();
            let at = rng(&mut s) as usize % b.len();
            b[at] = rng(&mut s) as u8;
            let cut = rng(&mut s) as usize % (b.len() + 1);
            if let Ok(bits) = Bits::parse(&b[..cut]) {
                let i = rng(&mut s) as usize % 4000;
                let _ = (
                    bits.rank(i),
                    bits.get(i),
                    bits.index(i),
                    bits.check(),
                    bits.iter().take(5000).count(),
                );
            }
        }
    }
}
