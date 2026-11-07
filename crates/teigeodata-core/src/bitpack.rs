//! Frame-of-reference bit-packing in lane-interleaved blocks (benchmark B1, D-073).
//!
//! A column of u32 values is cut into blocks of [`BLOCK_LEN`] = 128 values: 4 lanes of 32. A
//! block stores one base and one bit width `w`. Each lane packs its 32 values into `w` words, and
//! lanes are interleaved word by word, so every lane does the same shifts and the decoder's inner
//! loop runs over lanes: plain safe code that the compiler vectorises (the BP128 layout; Lemire &
//! Boytsov 2015). B1 measured 1024-value blocks (`FastLanes`, Afroozeh & Boncz 2023) too: they
//! decode 2.3× faster but are 6–41% larger on real columns and cost 300 KB more code.
//!
//! Value `j` of a block is in lane `j % 4`, row `j / 4`. Three codings:
//! - **FOR:** a value is `base + packed`; any value is read in O(1).
//! - **Delta:** `base` is the block's first value, and the packed values are zigzag deltas from
//!   the previous value (the first delta is 0). Sorted or spatially ordered columns (IDs, points in
//!   Hilbert order) shrink most; reading value `j` unpacks its block and sums its deltas up to `j`.
//! - **Ramp:** a value is `base + j + packed`; O(1), for dense sorted runs and near-identity
//!   permutations (D-075).
//!
//! Column layout, all little-endian:
//! ```text
//! count u32, lanes u8 (4), coding u8 (0 FOR, 1 delta, 2 ramp), reserved u16 (0)
//! bases   [u32; blocks]
//! offsets [u32; blocks]   word offset of each block in the payload
//! widths  [u8; blocks], zero padding to a multiple of 4
//! payload [u32]           block after block, w × 4 words each
//! ```
//! The lanes byte is kept so that a later layout is a new value, not a new format.
//! [`Packed::parse`] validates all of it once; reads after that cannot fail or panic.

use core::fmt;

/// Lanes per block.
const L: usize = 4;
/// Values per block.
pub const BLOCK_LEN: usize = 32 * L;

/// How values are coded inside a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    /// `base + packed`: O(1) random access.
    For = 0,
    /// Zigzag deltas from the block's first value.
    Delta = 1,
    /// `base + j + packed`, `j` the value's index in its block: FOR around a ramp of step 1.
    /// Dense sorted runs (serials 1, 2, 4, 5, …) and near-identity permutations pack to a few
    /// bits, or none, with O(1) access (D-075).
    Ramp = 2,
}

/// Why bytes are not a packed column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackError {
    /// Shorter than its header, directory or payload says.
    Truncated,
    /// Lanes are not 4, the coding is unknown, or reserved bytes are not zero.
    Header,
    /// A width above 32, an offset that does not follow from the widths, non-zero padding, or
    /// bytes after the payload.
    Directory,
    /// A delta block whose first delta is not 0: the base is the block's first value, so a
    /// block's first value reads the same through `get` and `block_first`.
    Block,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Truncated => "the packed column is truncated",
            Self::Header => "the packed column header is invalid",
            Self::Directory => "the packed column directory is inconsistent",
            Self::Block => "a delta block does not start at its base",
        })
    }
}

impl core::error::Error for PackError {}

const HEADER: usize = 8;

/// A validated packed column, borrowing its bytes.
#[derive(Clone, Copy, Debug)]
pub struct Packed<'a> {
    count: usize,
    coding: Coding,
    bases: &'a [[u8; 4]],
    offsets: &'a [[u8; 4]],
    widths: &'a [u8],
    payload: &'a [[u8; 4]],
}

fn le(b: [u8; 4]) -> u32 {
    u32::from_le_bytes(b)
}

#[cfg(any(feature = "alloc", test))]
const fn zigzag(d: u32) -> u32 {
    (d << 1) ^ (d.cast_signed() >> 31).cast_unsigned()
}

const fn unzigzag(z: u32) -> u32 {
    (z >> 1) ^ (z & 1).wrapping_neg()
}

/// One row of a block: value `R` of every lane. `W` and `R` are constants and `words` is an
/// array of exactly `W` rows, so every index and shift is a constant, no bounds check remains, and
/// the lane loop is straight vector code (B13: safe code, no `unsafe` needed).
#[inline(always)]
#[allow(
    clippy::inline_always,
    reason = "the 32 rows must fold into one unrolled block (B1)"
)]
#[allow(clippy::cast_possible_truncation, reason = "W ≤ 32, bit < 1024")]
fn row<const W: usize, const R: usize>(words: &[[[u8; 4]; L]; W], base: u32, out: &mut [u32; L]) {
    let (w, bit) = (W as u32, (R * W) as u32);
    let mask = u32::MAX >> (32 - w);
    let (k, s) = ((bit / 32) as usize, bit % 32);
    let zero = [[0; 4]; L];
    let lo = words.get(k).unwrap_or(&zero);
    if s + w <= 32 {
        for (o, x) in out.iter_mut().zip(lo) {
            *o = base.wrapping_add((le(*x) >> s) & mask);
        }
    } else {
        let hi = words.get(k + 1).unwrap_or(&zero);
        for ((o, x), y) in out.iter_mut().zip(lo).zip(hi) {
            *o = base.wrapping_add(((le(*x) >> s) | (le(*y) << (32 - s))) & mask);
        }
    }
}

/// Unpacks one block of `32 × L` values of width `W` from `words` (`W × L` of them) into `out`,
/// adding `base`. The 32 rows are written out, not looped, so the block is one straight run of
/// vector code, as hand-generated bit-packers are.
#[inline]
fn unpack_w<const W: usize>(words: &[[[u8; 4]; L]], base: u32, out: &mut [[u32; L]; 32]) {
    // `parse` checked that every block has its `W` rows; the fallback is never taken.
    let Some(words) = words.first_chunk::<W>().filter(|_| W > 0) else {
        *out = [[base; L]; 32];
        return;
    };
    let [
        r0,
        r1,
        r2,
        r3,
        r4,
        r5,
        r6,
        r7,
        r8,
        r9,
        r10,
        r11,
        r12,
        r13,
        r14,
        r15,
        r16,
        r17,
        r18,
        r19,
        r20,
        r21,
        r22,
        r23,
        r24,
        r25,
        r26,
        r27,
        r28,
        r29,
        r30,
        r31,
    ] = out;
    macro_rules! rows {
        ($($r:ident $n:literal)*) => { $(row::<W, $n>(words, base, $r);)* };
    }
    rows!(r0 0 r1 1 r2 2 r3 3 r4 4 r5 5 r6 6 r7 7 r8 8 r9 9 r10 10 r11 11 r12 12 r13 13 r14 14 r15 15
        r16 16 r17 17 r18 18 r19 19 r20 20 r21 21 r22 22 r23 23 r24 24 r25 25 r26 26 r27 27 r28 28
        r29 29 r30 30 r31 31);
}

/// Unpacks with the width chosen at run time: one `match` per block, then constant code. Kept
/// out of line: the full decode and `get` share one copy of the 33 unrolled widths (Z2).
#[inline(never)]
fn unpack(words: &[[[u8; 4]; L]], w: u32, base: u32, out: &mut [[u32; L]; 32]) {
    macro_rules! widths {
        ($($n:literal)*) => {
            match w {
                $($n => unpack_w::<$n>(words, base, out),)*
                _ => *out = [[base; L]; 32],
            }
        };
    }
    widths!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32);
}

impl<'a> Packed<'a> {
    /// Validates a packed column.
    ///
    /// # Errors
    /// [`PackError`] for any inconsistency.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, PackError> {
        let head: &[u8; HEADER] = bytes.first_chunk().ok_or(PackError::Truncated)?;
        let [c0, c1, c2, c3, lanes, coding, r0, r1] = *head;
        let count = usize::try_from(le([c0, c1, c2, c3])).map_err(|_| PackError::Truncated)?;
        if usize::from(lanes) != L {
            return Err(PackError::Header);
        }
        let coding = match coding {
            0 => Coding::For,
            1 => Coding::Delta,
            2 => Coding::Ramp,
            _ => return Err(PackError::Header),
        };
        if (r0, r1) != (0, 0) {
            return Err(PackError::Header);
        }
        let blocks = count.div_ceil(BLOCK_LEN);
        let rest = bytes.get(HEADER..).ok_or(PackError::Truncated)?;
        let (bases, rest) = split_words(rest, blocks)?;
        let (offsets, rest) = split_words(rest, blocks)?;
        let padded = blocks.next_multiple_of(4);
        let widths = rest.get(..blocks).ok_or(PackError::Truncated)?;
        let pad = rest.get(blocks..padded).ok_or(PackError::Truncated)?;
        let rest = rest.get(padded..).ok_or(PackError::Truncated)?;
        if pad.iter().any(|&b| b != 0) {
            return Err(PackError::Directory);
        }
        // Offsets must follow from the widths, so every block lies inside the payload.
        let mut next: u64 = 0;
        for (&w, off) in widths.iter().zip(offsets) {
            if w > 32 || u64::from(le(*off)) != next {
                return Err(PackError::Directory);
            }
            next += u64::from(w) * L as u64;
        }
        let (payload, tail) = rest.as_chunks::<4>();
        if !tail.is_empty() || payload.len() as u64 != next {
            return Err(PackError::Directory);
        }
        let column = Self {
            count,
            coding,
            bases,
            offsets,
            widths,
            payload,
        };
        // Canonical delta blocks: the first delta is 0 (O(1) per block, no unpacking).
        if coding == Coding::Delta
            && (0..blocks).any(|b| {
                let (words, width, _) = column.block_words(b);
                packed_at(words, width, 0) != 0
            })
        {
            return Err(PackError::Block);
        }
        Ok(column)
    }

    /// The number of values.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether the column is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The coding.
    #[must_use]
    pub const fn coding(&self) -> Coding {
        self.coding
    }

    /// The number of blocks.
    #[must_use]
    pub const fn blocks(&self) -> usize {
        self.widths.len()
    }

    fn block_words(&self, b: usize) -> (&'a [[u8; 4]], u32, u32) {
        let w = self.widths.get(b).copied().unwrap_or(0);
        let base = self.bases.get(b).copied().map_or(0, le);
        let off = self.offsets.get(b).copied().map_or(0, le) as usize;
        let n = usize::from(w) * L;
        let words = self.payload.get(off..off + n).unwrap_or_default();
        (words, u32::from(w), base)
    }

    /// Decodes block `b` into `out` and returns how many values it wrote: the block's values,
    /// cut to `out.len()`; 0 if `b` is past the end. An `out` of a whole block is fastest (the
    /// values are unpacked straight into it).
    pub fn decode_block(&self, b: usize, out: &mut [u32]) -> usize {
        let start = b.saturating_mul(BLOCK_LEN);
        let n = self
            .count
            .saturating_sub(start)
            .min(BLOCK_LEN)
            .min(out.len());
        if n == 0 {
            return 0;
        }
        let (words, w, base) = self.block_words(b);
        decode(words, w, base, self.coding, out);
        n
    }

    /// Value `i`, or `None` past the end. O(1) for [`Coding::For`]; for [`Coding::Delta`],
    /// unpacks its block and sums the deltas up to `i`.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<u32> {
        if i >= self.count {
            return None;
        }
        let (block, j) = (i / BLOCK_LEN, i % BLOCK_LEN);
        let (words, width, base) = self.block_words(block);
        Some(base.wrapping_add(match self.coding {
            Coding::For => packed_at(words, width, j),
            Coding::Delta => delta_sum(words, width, j),
            #[allow(clippy::cast_possible_truncation, reason = "j < 128")]
            Coding::Ramp => packed_at(words, width, j).wrapping_add(j as u32),
        }))
    }

    /// The first value of block `b` in O(1), without unpacking it; `None` past the end. On a
    /// sorted column these are the keys of a search over blocks.
    #[must_use]
    pub fn block_first(&self, b: usize) -> Option<u32> {
        if b >= self.blocks() {
            return None;
        }
        let (words, width, base) = self.block_words(b);
        Some(match self.coding {
            Coding::For | Coding::Ramp => base.wrapping_add(packed_at(words, width, 0)),
            Coding::Delta => base,
        })
    }

    /// The value of every entry of block `b` if they are all equal and that is known without
    /// unpacking (a width-0 FOR or delta block); `None` otherwise.
    #[must_use]
    pub fn block_constant(&self, b: usize) -> Option<u32> {
        let (_, width, base) = self.block_words(b);
        (b < self.blocks() && width == 0 && self.coding != Coding::Ramp).then_some(base)
    }

    /// The values in `range`, cut to [`Packed::len`], decoded a block at a time: the way to
    /// stream a column or read a run (a children list) without a buffer of its own.
    #[must_use]
    pub fn values(&self, range: core::ops::Range<usize>) -> Values<'a> {
        let end = range.end.min(self.count);
        Values {
            column: *self,
            next: range.start.min(end),
            end,
            block: usize::MAX,
            buf: [0; BLOCK_LEN],
        }
    }

    /// Decodes values into `out` from the first; returns how many it wrote, at most
    /// [`Packed::len`]. Rounding `out` up to a whole block lets the last block be unpacked in
    /// place too; entries of `out` past the returned count may then be overwritten.
    pub fn decode_all(&self, out: &mut [u32]) -> usize {
        let bl = BLOCK_LEN;
        let mut n = 0;
        for (b, chunk) in out.chunks_mut(bl).enumerate().take(self.blocks()) {
            n += self.decode_block(b, chunk);
        }
        n
    }
}

/// Packed value `j` of a block: row `j / 4` of lane `j % 4`, at bit `row × width` of that lane.
fn packed_at(words: &[[u8; 4]], width: u32, j: usize) -> u32 {
    if width == 0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, reason = "row < 32")]
    let bit = (j / L) as u32 * width;
    let (word, shift) = ((bit / 32) as usize, bit % 32);
    let load = |k: usize| words.get(k * L + j % L).copied().map_or(0, le);
    let lo = load(word) >> shift;
    let v = if shift + width <= 32 {
        lo
    } else {
        lo | (load(word + 1) << (32 - shift))
    };
    v & (u32::MAX >> (32 - width))
}

/// The sum of the first `j + 1` deltas of a block (the first is 0): the block is unpacked with
/// the vector code, then summed; addition is associative, so the sum vectorises too, unlike the
/// prefix sum of a full decode.
fn delta_sum(words: &[[u8; 4]], w: u32, j: usize) -> u32 {
    let mut block = [[0_u32; L]; 32];
    unpack(words.as_chunks::<L>().0, w, 0, &mut block);
    let deltas = block.as_flattened();
    deltas
        .get(..=j)
        .unwrap_or(deltas)
        .iter()
        .fold(0, |a, &z| a.wrapping_add(unzigzag(z)))
}

/// An iterator over a run of a column ([`Packed::values`]).
#[derive(Clone, Debug)]
pub struct Values<'a> {
    column: Packed<'a>,
    next: usize,
    end: usize,
    block: usize,
    buf: [u32; BLOCK_LEN],
}

impl Iterator for Values<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.next >= self.end {
            return None;
        }
        let (b, j) = (self.next / BLOCK_LEN, self.next % BLOCK_LEN);
        if b != self.block {
            self.column.decode_block(b, &mut self.buf);
            self.block = b;
        }
        self.next += 1;
        self.buf.get(j).copied()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.end - self.next;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Values<'_> {}

/// An i32 as an order-preserving u32 (`i32::MIN` → 0), so signed columns pack like unsigned.
#[must_use]
pub const fn bias(v: i32) -> u32 {
    v.cast_unsigned() ^ 0x8000_0000
}

/// The inverse of [`bias`].
#[must_use]
pub const fn unbias(v: u32) -> i32 {
    (v ^ 0x8000_0000).cast_signed()
}

fn split_words(b: &[u8], n: usize) -> Result<(&[[u8; 4]], &[u8]), PackError> {
    let len = n.checked_mul(4).ok_or(PackError::Truncated)?;
    let (head, rest) = b.split_at_checked(len).ok_or(PackError::Truncated)?;
    Ok((head.as_chunks::<4>().0, rest))
}

/// Decodes one block into `out`: straight into it when it holds a whole block (FOR adds the
/// base while unpacking; delta then runs its prefix sum in place), through a scratch block
/// otherwise (the last, short block).
fn decode(words: &[[u8; 4]], width: u32, base: u32, coding: Coding, out: &mut [u32]) {
    let rows = words.as_chunks::<L>().0;
    let fill = |block: &mut [[u32; L]; 32]| {
        let for_base = if coding == Coding::Delta { 0 } else { base };
        unpack(rows, width, for_base, block);
        if coding == Coding::Delta {
            // A prefix sum row by row: the sums inside a row of 4 are independent, so only one
            // addition per row (the carry) is on the dependency chain, not one per value.
            let mut carry = base;
            for row in block.iter_mut() {
                let [a, b, c, d] = row.map(unzigzag);
                let ab = a.wrapping_add(b);
                let abc = ab.wrapping_add(c);
                *row = [a, ab, abc, abc.wrapping_add(d)].map(|v| carry.wrapping_add(v));
                carry = row[3];
            }
        } else if coding == Coding::Ramp {
            for (j, v) in (0_u32..).zip(block.as_flattened_mut()) {
                *v = v.wrapping_add(j);
            }
        }
    };
    if let Some(block) = out.as_chunks_mut::<L>().0.first_chunk_mut::<32>() {
        fill(block);
    } else {
        let mut scratch = [[0_u32; L]; 32];
        fill(&mut scratch);
        for (dst, v) in out.iter_mut().zip(scratch.as_flattened()) {
            *dst = *v;
        }
    }
}

/// Encodes a column (feature `alloc`).
#[cfg(feature = "alloc")]
#[must_use]
pub fn encode(values: &[u32], coding: Coding) -> alloc::vec::Vec<u8> {
    use alloc::vec::Vec;
    let bl = BLOCK_LEN;
    let blocks = values.len().div_ceil(bl);
    let mut bases = Vec::with_capacity(blocks);
    let mut widths = Vec::with_capacity(blocks);
    let mut payload: Vec<u32> = Vec::new();
    let mut offsets = Vec::with_capacity(blocks);
    let mut packed = alloc::vec![0_u32; bl];
    for chunk in values.chunks(bl) {
        packed.fill(0);
        let base = match coding {
            Coding::For => {
                let min = chunk.iter().copied().min().unwrap_or(0);
                for (p, v) in packed.iter_mut().zip(chunk) {
                    *p = v.wrapping_sub(min);
                }
                min
            }
            Coding::Delta => {
                let first = chunk.first().copied().unwrap_or(0);
                let mut prev = first;
                for (p, v) in packed.iter_mut().zip(chunk) {
                    *p = zigzag(v.wrapping_sub(prev));
                    prev = *v;
                }
                first
            }
            Coding::Ramp => {
                let ramp = |(j, v): (u32, &u32)| v.wrapping_sub(j);
                let min = (0_u32..).zip(chunk).map(ramp).min().unwrap_or(0);
                for (p, d) in packed.iter_mut().zip((0_u32..).zip(chunk).map(ramp)) {
                    *p = d.wrapping_sub(min);
                }
                min
            }
        };
        let width = 32 - packed.iter().fold(0, |a, v| a | v).leading_zeros();
        let start = payload.len();
        offsets.push(u32::try_from(start).unwrap_or(u32::MAX));
        payload.resize(start + width as usize * L, 0);
        if width > 0 {
            let words = payload.get_mut(start..).unwrap_or_default();
            for (j, v) in packed.iter().enumerate() {
                let (row, lane) = (j / L, j % L);
                #[allow(clippy::cast_possible_truncation, reason = "row < 32")]
                let bit = row as u32 * width;
                let (word, shift) = ((bit / 32) as usize, bit % 32);
                if let Some(x) = words.get_mut(word * L + lane) {
                    *x |= v << shift;
                }
                if shift + width > 32
                    && let Some(x) = words.get_mut((word + 1) * L + lane)
                {
                    *x |= v >> (32 - shift);
                }
            }
        }
        bases.push(base);
        #[allow(clippy::cast_possible_truncation, reason = "w ≤ 32")]
        widths.push(width as u8);
    }
    let mut out = Vec::with_capacity(HEADER + blocks * 9 + 4 + payload.len() * 4);
    out.extend_from_slice(
        &u32::try_from(values.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    #[allow(clippy::cast_possible_truncation, reason = "L = 4")]
    out.extend_from_slice(&[L as u8, coding as u8, 0, 0]);
    for v in bases.iter().chain(&offsets) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&widths);
    out.resize(out.len() + (blocks.next_multiple_of(4) - blocks), 0);
    for v in &payload {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encodes a column with the smaller of FOR and ramp (feature `alloc`): both read one value
/// in O(1); FOR on a tie. For columns read on every lookup.
#[cfg(feature = "alloc")]
#[must_use]
pub fn encode_direct(values: &[u32]) -> alloc::vec::Vec<u8> {
    let (fixed, ramp) = (encode(values, Coding::For), encode(values, Coding::Ramp));
    if ramp.len() < fixed.len() {
        ramp
    } else {
        fixed
    }
}

/// Encodes a column with the coding D-073 and D-075 pick (feature `alloc`):
/// [`encode_direct`]'s, unless delta is at least 10% smaller than that, since delta reads a
/// single value 4–5× slower.
#[cfg(feature = "alloc")]
#[must_use]
pub fn encode_auto(values: &[u32]) -> alloc::vec::Vec<u8> {
    let direct = encode_direct(values);
    let delta = encode(values, Coding::Delta);
    if delta.len() * 10 <= direct.len() * 9 {
        delta
    } else {
        direct
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "test inputs are cut from a 64-bit generator on purpose"
)]
mod tests {
    use super::{BLOCK_LEN, Coding, PackError, Packed, encode, encode_auto, unzigzag, zigzag};
    use std::vec::Vec;

    fn rng(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    fn check(values: &[u32]) {
        {
            for coding in [Coding::For, Coding::Delta, Coding::Ramp] {
                let bytes = encode(values, coding);
                let p = Packed::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(p.len(), values.len());
                let mut out = std::vec![0; values.len().next_multiple_of(BLOCK_LEN)];
                assert_eq!(p.decode_all(&mut out), values.len());
                assert_eq!(&out[..values.len()], values, "{coding:?}");
                // An exact-length buffer (the short last block goes through scratch), and a
                // short one (cut, never overrun).
                let mut exact = std::vec![0; values.len()];
                assert_eq!(p.decode_all(&mut exact), values.len());
                assert_eq!(exact, values, "{coding:?} exact");
                let cut = values.len() / 3;
                let mut short = std::vec![0; cut];
                assert_eq!(p.decode_all(&mut short), cut);
                assert_eq!(short, values[..cut], "{coding:?} short");
                for (i, v) in values.iter().enumerate().step_by(7) {
                    assert_eq!(p.get(i), Some(*v), "{coding:?} at {i}");
                }
                assert_eq!(p.get(values.len()), None);
                for b in 0..=p.blocks() {
                    assert_eq!(p.block_first(b), values.get(b * BLOCK_LEN).copied());
                    if let Some(c) = p.block_constant(b) {
                        let block = values.iter().skip(b * BLOCK_LEN).take(BLOCK_LEN);
                        assert!(block.clone().all(|&v| v == c), "{coding:?} block {b}");
                    }
                }
            }
        }
    }

    #[test]
    fn zigzag_round_trips() {
        for v in [0, 1, u32::MAX, 0x8000_0000, 0x7fff_ffff, 12345] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
        assert_eq!(zigzag(u32::MAX), 1); // -1
        assert_eq!(zigzag(1), 2);
    }

    #[test]
    fn round_trips_every_width_and_length() {
        let mut s = 0x2545_f491_4f6c_dd1d;
        // Every width 0–32, at lengths around block edges.
        for w in 0..=32_u32 {
            let mask = if w == 0 { 0 } else { u32::MAX >> (32 - w) };
            for n in [0, 1, 31, 127, 128, 129, 1023, 1024, 1025, 3000] {
                let v: Vec<u32> = (0..n).map(|_| (rng(&mut s) as u32) & mask).collect();
                check(&v);
            }
        }
        // Sorted, constant, extremes, and wrapping deltas.
        check(&(0..5000).map(|i| i * 3 + 1_000_000).collect::<Vec<_>>());
        check(&[7; 2000]);
        check(&(0..3000).collect::<Vec<_>>()); // width-0 ramp blocks
        check(&[0, u32::MAX, 0, u32::MAX, 1]);
        check(
            &(0..3000_u32)
                .map(|i| i.wrapping_mul(2_654_435_761_u32).rotate_left(i % 31))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn a_delta_block_must_start_at_its_base() {
        let values: Vec<u32> = (0..200).map(|i| i * 7 + i % 3).collect();
        let good = encode(&values, Coding::Delta);
        let p = Packed::parse(&good).unwrap();
        assert_eq!(p.block_first(1), p.get(BLOCK_LEN));
        // Two blocks: the payload starts after the header, two bases, two offsets and the
        // padded widths; its first word holds lane 0's first delta in its low bits.
        let mut bad = good.clone();
        bad[8 + 8 + 8 + 4] |= 1;
        assert_eq!(Packed::parse(&bad).err(), Some(PackError::Block));
        // FOR has no such rule: the same edit is a valid FOR column.
        let mut f = encode(&values, Coding::For);
        f[8 + 8 + 8 + 4] ^= 1;
        assert!(Packed::parse(&f).is_ok());
    }

    #[test]
    fn delta_is_smaller_on_sorted_values() {
        let v: Vec<u32> = (0..10_000).map(|i| 50_000_000 + i * 17).collect();
        let f = encode(&v, Coding::For).len();
        let d = encode(&v, Coding::Delta).len();
        // Steps of 17 need 12 bits under FOR (127 × 17 = 2159) and 6 under delta (zigzag 34).
        assert!(d * 10 < f * 6, "delta {d} vs for {f}");
    }

    #[test]
    fn ramp_packs_runs_to_nothing() {
        // The identity, and a dense run with a few gaps: ramp, O(1), nearly no payload.
        let identity: Vec<u32> = (0..10_000).collect();
        let gaps: Vec<u32> = (0..10_000).map(|i| 7 + i + i / 1000).collect();
        for v in [&identity, &gaps] {
            let bytes = encode_auto(v);
            let p = Packed::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(p.coding(), Coding::Ramp);
            // Header, bases, offsets and widths only, plus one word per lane for a gap block.
            assert!(
                bytes.len() < 8 + 9 * p.blocks() + 4 + 16 * 10,
                "{}",
                bytes.len()
            );
            assert_eq!(p.get(9_999), v.last().copied());
        }
        // Values below their index wrap around; still exact, never compact.
        let down: Vec<u32> = (0..300).rev().collect();
        let p = encode(&down, Coding::Ramp);
        let mut out = std::vec![0; 300];
        assert_eq!(Packed::parse(&p).map(|c| c.decode_all(&mut out)), Ok(300));
        assert_eq!(out, down);
    }

    #[test]
    fn auto_picks_delta_only_when_it_pays() {
        let sorted: Vec<u32> = (0..10_000).map(|i| 50_000_000 + i * 17).collect();
        let auto = |v: &[u32]| Packed::parse(&encode_auto(v)).map(|p| p.coding());
        assert_eq!(auto(&sorted), Ok(Coding::Delta));
        let mut s = 3;
        let noise: Vec<u32> = (0..10_000).map(|_| rng(&mut s) as u32 & 0xfff).collect();
        assert_eq!(auto(&noise), Ok(Coding::For));
        // Delta a little smaller (under 10%): FOR, for its faster reads. Per 128-value block,
        // noise costs FOR 12 bits and delta 13; a ramp of step 30 costs FOR 12 and delta 6. One
        // ramp per four noise blocks: delta saves (6 - 4) / 60 = 3.3%.
        let mixed: Vec<u32> = (0..50 * BLOCK_LEN)
            .map(|i| {
                let (block, j) = (i / BLOCK_LEN, (i % BLOCK_LEN) as u32);
                if block % 5 == 0 {
                    j * 30
                } else {
                    (rng(&mut s) as u32 & 0xfff) | if j == 0 { 0xfff } else { 0 }
                }
            })
            .collect();
        let (f, d) = (
            encode(&mixed, Coding::For).len(),
            encode(&mixed, Coding::Delta).len(),
        );
        assert!(
            d < f && d * 10 > f * 9,
            "the case must be in the band: {d} vs {f}"
        );
        assert_eq!(auto(&mixed), Ok(Coding::For));
    }

    #[test]
    fn values_stream_any_range() {
        let v: Vec<u32> = (0..1000_u32)
            .map(|i| i.wrapping_mul(2_654_435_761))
            .collect();
        for coding in [Coding::For, Coding::Delta, Coding::Ramp] {
            let bytes = encode(&v, coding);
            let p = Packed::parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
            for (a, b) in [
                (0, 1000),
                (0, 0),
                (5, 6),
                (127, 129),
                (300, 2000),
                (999, 1000),
                (1200, 1300),
            ] {
                let got: Vec<u32> = p.values(a..b).collect();
                let want = v.get(a.min(1000)..b.min(1000)).unwrap_or_default();
                assert_eq!(got, want, "{coding:?} {a}..{b}");
                assert_eq!(p.values(a..b).len(), want.len());
            }
        }
        assert_eq!(super::unbias(super::bias(-5)), -5);
        assert!(super::bias(-1) < super::bias(0) && super::bias(i32::MIN) == 0);
    }

    #[test]
    fn parse_rejects() {
        let good = encode(&(0..300).collect::<Vec<_>>(), Coding::For);
        assert!(Packed::parse(&good).is_ok());
        assert_eq!(Packed::parse(&good[..7]).err(), Some(PackError::Truncated));
        assert_eq!(
            Packed::parse(&good[..good.len() - 1]).err(),
            Some(PackError::Directory)
        );
        let mut extra = good.clone();
        extra.extend_from_slice(&[0; 4]);
        assert_eq!(Packed::parse(&extra).err(), Some(PackError::Directory));
        let mut b = good.clone();
        b[4] = 8;
        assert_eq!(Packed::parse(&b).err(), Some(PackError::Header));
        b[4] = 32; // the 1024-value layout B1 measured and rejected
        assert_eq!(Packed::parse(&b).err(), Some(PackError::Header));
        let mut b = good.clone();
        b[5] = 3;
        assert_eq!(Packed::parse(&b).err(), Some(PackError::Header));
        // Three blocks: widths start at 8 + 12 + 12 = 32.
        let mut b = good.clone();
        b[32] = 33;
        assert_eq!(Packed::parse(&b).err(), Some(PackError::Directory));
        let mut b = good;
        b[8 + 12 + 4] ^= 1; // the second block's offset
        assert_eq!(Packed::parse(&b).err(), Some(PackError::Directory));
    }

    #[test]
    fn parse_never_panics() {
        let values: Vec<u32> = (0..2000).map(|i| i * 7).collect();
        let bases = [
            encode(&values, Coding::For),
            encode(&values, Coding::Delta),
            encode(&values, Coding::Ramp),
        ];
        let mut s = 1;
        let mut out = std::vec![0; 4096];
        for k in 0..20_000 {
            let mut b = bases[k % 3].clone();
            let at = rng(&mut s) as usize % b.len();
            b[at] = rng(&mut s) as u8;
            let cut = rng(&mut s) as usize % (b.len() + 1);
            if let Ok(p) = Packed::parse(&b[..cut]) {
                let _ = p.decode_all(&mut out);
                let _ = p.get(rng(&mut s) as usize % (p.len() + 1));
            }
        }
    }
}
