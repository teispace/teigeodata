//! `BLOCK_HASHES` (kind 15): lazy verification of large sections, a 16 KiB block at a time
//! (B12, D-078). For each section of [`MIN_COVERED`] bytes or more, the list holds the BLAKE3
//! chaining value (CV) of each block: exactly the leaves of the section's own BLAKE3 tree, so
//! merging them gives the hash already in the section table. The list needs no trust root of
//! its own, and the writer gets it and the section hash in one pass.
//!
//! Layout ([`Encoding::RAW`], little-endian; `item_count` is the number of CVs):
//! ```text
//! block_log2 u8 (14), reserved [u8; 3] (0), entries u32, cvs u64
//! directory [{ kind u32, reserved u32 (0), first u64 }; entries]   kinds increasing
//! cvs       [[u8; 32]; cvs]   cvs[first + i] is block i of that section
//! ```
//! Canonical: there is a list exactly when some section other than the list is at least
//! [`MIN_COVERED`] bytes, and it covers exactly those sections, `first` running from 0 without
//! gaps. [`Pack::open`](crate::Pack::open) checks that; [`Pack::blocks`](crate::Pack::blocks)
//! proves one section's CVs against its hash; [`Blocks::verify`] then checks a block.

use core::ops::Range;
use core::sync::atomic::{AtomicUsize, Ordering};

use blake3::hazmat::{
    ChainingValue, HasherExt, Mode, left_subtree_len, merge_subtrees_non_root, merge_subtrees_root,
};

use crate::{Encoding, Error, Kind, Section};

/// log₂ of the block length.
pub const BLOCK_LOG2: u8 = 14;
/// The block length: 16 KiB.
pub const BLOCK: usize = 1 << BLOCK_LOG2;
/// Sections this long or longer are covered: 64 blocks, 1 MiB.
pub const MIN_COVERED: u64 = 1 << 20;

const HEADER: usize = 16;
const ENTRY: usize = 16;

fn fail(problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::BLOCK_HASHES.0,
        column: 0,
        problem,
    }
}

/// Whether a section of `kind` and `length` must be covered.
pub(crate) fn covered(kind: Kind, length: u64) -> bool {
    kind != Kind::BLOCK_HASHES && length >= MIN_COVERED
}

/// The number of blocks of a section of `length` bytes.
pub(crate) const fn blocks_of(length: u64) -> u64 {
    length.div_ceil(BLOCK as u64)
}

/// The CV of block `i`, whose bytes are `block` (at most [`BLOCK`], not empty).
#[must_use]
pub fn leaf(i: u64, block: &[u8]) -> ChainingValue {
    let mut h = blake3::Hasher::new();
    h.set_input_offset(i << BLOCK_LOG2);
    h.update(block);
    h.finalize_non_root()
}

/// The CV of the subtree over `cvs`, which cover `len` bytes (two blocks or more).
fn subtree(cvs: &[ChainingValue], len: u64) -> Option<ChainingValue> {
    if let [only] = cvs {
        return Some(*only);
    }
    let (l, r, left) = split(cvs, len)?;
    Some(merge_subtrees_non_root(
        &subtree(l, left)?,
        &subtree(r, len - left)?,
        Mode::Hash,
    ))
}

/// The left and right halves of the tree over `cvs` (`len` bytes, two blocks or more), and the
/// left half's length: the largest power of two below `len`, as BLAKE3 splits.
fn split(cvs: &[ChainingValue], len: u64) -> Option<(&[ChainingValue], &[ChainingValue], u64)> {
    if cvs.len() < 2 || blocks_of(len) != cvs.len() as u64 {
        return None;
    }
    let left = left_subtree_len(len);
    let (l, r) = cvs.split_at_checked(usize::try_from(left >> BLOCK_LOG2).ok()?)?;
    Some((l, r, left))
}

/// The root hash of a section of `len` bytes from its block CVs; `None` if they do not fit
/// (fewer than two, or not one per block).
#[must_use]
pub fn root(cvs: &[ChainingValue], len: u64) -> Option<[u8; 32]> {
    let (l, r, left) = split(cvs, len)?;
    Some(*merge_subtrees_root(&subtree(l, left)?, &subtree(r, len - left)?, Mode::Hash).as_bytes())
}

/// A parsed `BLOCK_HASHES` section.
#[derive(Clone, Copy, Debug)]
pub(crate) struct List<'a> {
    entries: &'a [[u8; ENTRY]],
    cvs: &'a [[u8; 32]],
}

fn entry(e: [u8; ENTRY]) -> (u32, u32, u64) {
    let [k0, k1, k2, k3, r0, r1, r2, r3, f @ ..] = e;
    (
        u32::from_le_bytes([k0, k1, k2, k3]),
        u32::from_le_bytes([r0, r1, r2, r3]),
        u64::from_le_bytes(f),
    )
}

impl<'a> List<'a> {
    fn parse(section: &Section<'a>) -> Result<Self, Error> {
        if section.encoding() != Encoding::RAW {
            return Err(fail("not a raw section"));
        }
        let (&[log2, r0, r1, r2, e0, e1, e2, e3, c @ ..], rest) = section
            .bytes()
            .split_first_chunk::<HEADER>()
            .ok_or(fail("truncated"))?;
        if log2 != BLOCK_LOG2 || [r0, r1, r2] != [0; 3] {
            return Err(fail("the block size or a reserved byte is wrong"));
        }
        let entries =
            usize::try_from(u32::from_le_bytes([e0, e1, e2, e3])).map_err(|_| fail("too long"))?;
        let cvs = u64::from_le_bytes(c);
        if section.item_count() != cvs {
            return Err(fail("the item count is not the number of block hashes"));
        }
        let (dir, cv_bytes) = rest
            .split_at_checked(entries.checked_mul(ENTRY).ok_or(fail("too long"))?)
            .ok_or(fail("truncated"))?;
        let (cv_list, tail) = cv_bytes.as_chunks::<32>();
        if !tail.is_empty() || cv_list.len() as u64 != cvs {
            return Err(fail("the length does not match the counts"));
        }
        Ok(Self {
            entries: dir.as_chunks::<ENTRY>().0,
            cvs: cv_list,
        })
    }

    /// The CVs of section `kind`.
    fn cvs_of(&self, kind: Kind, length: u64) -> Option<&'a [[u8; 32]]> {
        let e = self
            .entries
            .iter()
            .map(|&e| entry(e))
            .find(|e| e.0 == kind.0)?;
        let first = usize::try_from(e.2).ok()?;
        let n = usize::try_from(blocks_of(length)).ok()?;
        self.cvs.get(first..first.checked_add(n)?)
    }
}

/// Checks the list's structure and coverage against the pack's sections, in O(sections):
/// [`Pack::open`](crate::Pack::open) calls it.
pub(crate) fn check<'a>(
    list: Option<Section<'a>>,
    mut sections: impl Iterator<Item = Section<'a>>,
) -> Result<(), Error> {
    let Some(list) = list else {
        return if sections.any(|s| covered(s.kind(), s.bytes().len() as u64)) {
            Err(fail("a section of 1 MiB or more has no block hashes"))
        } else {
            Ok(())
        };
    };
    let parsed = List::parse(&list)?;
    let mut entries = parsed.entries.iter().map(|&e| entry(e));
    let mut next = 0_u64;
    for s in sections.filter(|s| covered(s.kind(), s.bytes().len() as u64)) {
        match entries.next() {
            Some((kind, 0, first)) if kind == s.kind().0 && first == next => {
                next = next
                    .checked_add(blocks_of(s.bytes().len() as u64))
                    .ok_or(fail("too many blocks"))?;
            }
            _ => {
                return Err(fail(
                    "the entries do not match the sections of 1 MiB or more",
                ));
            }
        }
    }
    if entries.next().is_some() || next == 0 || next != parsed.cvs.len() as u64 {
        return Err(fail(
            "the entries do not match the sections of 1 MiB or more",
        ));
    }
    Ok(())
}

/// One covered section, its block hashes proven against its section hash.
#[derive(Clone, Copy, Debug)]
pub struct Blocks<'a, 'm> {
    section: Section<'a>,
    cvs: &'a [[u8; 32]],
    memo: Option<&'m [AtomicUsize]>,
}

impl<'a> Blocks<'a, '_> {
    /// Finds and proves `section`'s block hashes in `list`: O(blocks) merges, no hashing of
    /// the section. `None` if the section is not covered.
    pub(crate) fn prove(
        section: Section<'a>,
        list: Option<Section<'a>>,
    ) -> Result<Option<Self>, Error> {
        let len = section.bytes().len() as u64;
        if !covered(section.kind(), len) {
            return Ok(None);
        }
        let list = List::parse(&list.ok_or(fail("missing"))?)?;
        let cvs = list
            .cvs_of(section.kind(), len)
            .ok_or(fail("the section has no entry"))?;
        if root(cvs, len).as_ref() != Some(section.hash()) {
            return Err(Error::SectionHash {
                kind: section.kind().0,
            });
        }
        Ok(Some(Self {
            section,
            cvs,
            memo: None,
        }))
    }

    /// Remembers verified blocks in `memo`, one bit per block ([`Blocks::memo_words`] words),
    /// shared by every reader of the pack; with a shorter memo the blocks past it are checked on
    /// every read. Threads that verify a block at once all succeed: no lock is needed.
    #[must_use]
    pub fn with_memo<'n>(self, memo: &'n [AtomicUsize]) -> Blocks<'a, 'n> {
        Blocks {
            section: self.section,
            cvs: self.cvs,
            memo: Some(memo),
        }
    }

    /// The memo words a section needs: one bit per block.
    #[must_use]
    pub const fn memo_words(&self) -> usize {
        self.cvs.len().div_ceil(usize::BITS as usize)
    }

    /// The number of blocks.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.cvs.len()
    }

    /// Whether there are none (never, for a covered section).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.cvs.is_empty()
    }

    fn bit(&self, i: usize) -> Option<(&AtomicUsize, usize)> {
        let w = self.memo?.get(i / usize::BITS as usize)?;
        Some((w, 1 << (i % usize::BITS as usize)))
    }

    /// Block `i`'s bytes, checked against its hash (once, with a memo).
    ///
    /// # Errors
    /// [`Error::SectionHash`] if the block differs; [`Error::Length`] past the last block.
    pub fn verify(&self, i: usize) -> Result<&'a [u8], Error> {
        let bytes = self.section.bytes();
        let start = i.checked_mul(BLOCK).ok_or(Error::Length)?;
        let block = bytes
            .get(start..start.saturating_add(BLOCK).min(bytes.len()))
            .filter(|b| !b.is_empty())
            .ok_or(Error::Length)?;
        let memo = self.bit(i);
        if memo.is_some_and(|(w, m)| w.load(Ordering::Acquire) & m != 0) {
            return Ok(block);
        }
        if self.cvs.get(i) != Some(&leaf(i as u64, block)) {
            return Err(Error::SectionHash {
                kind: self.section.kind().0,
            });
        }
        if let Some((w, m)) = memo {
            w.fetch_or(m, Ordering::Release);
        }
        Ok(block)
    }

    /// The bytes `range` of the section, each block they touch checked.
    ///
    /// # Errors
    /// As [`Blocks::verify`]; [`Error::Length`] if the range is outside the section.
    pub fn bytes(&self, range: Range<usize>) -> Result<&'a [u8], Error> {
        let all = self.section.bytes();
        let out = all.get(range.clone()).ok_or(Error::Length)?;
        if !range.is_empty() {
            for i in range.start / BLOCK..=(range.end - 1) / BLOCK {
                self.verify(i)?;
            }
        }
        Ok(out)
    }
}

/// The `item_count` of a `BLOCK_HASHES` body from [`encode`]: its number of chaining values, read
/// from its header (0 if the header is short).
#[must_use]
pub fn item_count(list: &[u8]) -> u64 {
    list.get(8..16)
        .and_then(|b| b.try_into().ok())
        .map_or(0, u64::from_le_bytes)
}

/// Encodes `BLOCK_HASHES` for a pack's sections (feature `write`): `None` when no section is
/// covered. Hashes each covered section once, a block at a time.
///
/// # Errors
/// [`Error::Write`] if a kind is repeated or there are more than 2³² covered sections.
#[cfg(feature = "write")]
pub fn encode(sections: &[(Kind, &[u8])]) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
    use alloc::vec::Vec;
    let mut covered_ones: Vec<(Kind, &[u8])> = sections
        .iter()
        .copied()
        .filter(|(k, b)| covered(*k, b.len() as u64))
        .collect();
    covered_ones.sort_by_key(|c| c.0);
    if covered_ones
        .windows(2)
        .any(|w| w.first().map(|a| a.0) == w.get(1).map(|b| b.0))
    {
        return Err(Error::Write("a section kind was given twice"));
    }
    if covered_ones.is_empty() {
        return Ok(None);
    }
    let entries =
        u32::try_from(covered_ones.len()).map_err(|_| Error::Write("too many sections"))?;
    let total: u64 = covered_ones
        .iter()
        .map(|c| blocks_of(c.1.len() as u64))
        .sum();
    let mut out = Vec::with_capacity(
        HEADER + covered_ones.len() * ENTRY + 32 * usize::try_from(total).unwrap_or(0),
    );
    out.extend_from_slice(&[BLOCK_LOG2, 0, 0, 0]);
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    let mut first = 0_u64;
    for (kind, bytes) in &covered_ones {
        out.extend_from_slice(&kind.0.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&first.to_le_bytes());
        first += blocks_of(bytes.len() as u64);
    }
    for (_, bytes) in &covered_ones {
        for (i, block) in bytes.chunks(BLOCK).enumerate() {
            out.extend_from_slice(&leaf(i as u64, block));
        }
    }
    Ok(Some(out))
}

/// Proves every entry of a `BLOCK_HASHES` body against the sections' hashes (feature `write`):
/// the writer's check that the list it is given is the right one. O(blocks) merges.
#[cfg(feature = "write")]
pub(crate) fn prove_all(list: &[u8], sections: &[(Kind, u64, [u8; 32])]) -> Result<(), Error> {
    let bad = Error::Write("the block hashes do not match the sections");
    let (&[_, _, _, _, _, _, _, _, ..], rest) = list.split_first_chunk::<HEADER>().ok_or(bad)?;
    let entries = sections.iter().filter(|s| covered(s.0, s.1)).count();
    let (dir, cvs) = rest.split_at_checked(entries * ENTRY).ok_or(bad)?;
    let cvs = cvs.as_chunks::<32>().0;
    for (&e, (kind, len, hash)) in dir
        .as_chunks::<ENTRY>()
        .0
        .iter()
        .zip(sections.iter().filter(|s| covered(s.0, s.1)))
    {
        let (k, _, first) = entry(e);
        let first = usize::try_from(first).map_err(|_| bad)?;
        let n = usize::try_from(blocks_of(*len)).map_err(|_| bad)?;
        let slice = cvs.get(first..first + n).ok_or(bad)?;
        if k != kind.0 || root(slice, *len).as_ref() != Some(hash) {
            return Err(bad);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "test inputs sized here"
)]
mod tests {
    use super::{BLOCK, leaf, root};
    use std::vec::Vec;

    /// The merge agrees with `blake3::hash` at every length class: block boundaries, a byte
    /// either side, odd block counts, powers of two and the official test-vector lengths.
    #[test]
    fn root_equals_the_hash() {
        let data: Vec<u8> = (0..(70 * BLOCK + 1000)).map(|i| (i % 251) as u8).collect();
        // Under Miri, hashing is about a million times slower: an even count, an odd count and
        // a partial last block take every merge path (8 minutes for 2 to 5 blocks, 2026-11-07).
        let (blocks, extra): (&[usize], &[usize]) = if cfg!(miri) {
            (&[2], &[BLOCK + 1, 3 * BLOCK + 1])
        } else {
            (
                &[2, 3, 4, 5, 7, 8, 9, 16, 17, 31, 63, 64, 65, 70],
                &[BLOCK + 1, 2 * BLOCK + 1023, 31_744 + BLOCK, 102_400],
            )
        };
        let mut lens: Vec<usize> = Vec::new();
        for b in blocks {
            lens.extend([b * BLOCK - 1, b * BLOCK, b * BLOCK + 1]);
        }
        lens.extend(extra);
        for len in lens {
            let bytes = &data[..len];
            let cvs: Vec<[u8; 32]> = bytes
                .chunks(BLOCK)
                .enumerate()
                .map(|(i, b)| leaf(i as u64, b))
                .collect();
            assert_eq!(
                root(&cvs, len as u64),
                Some(*blake3::hash(bytes).as_bytes()),
                "{len}"
            );
            // A wrong CV or a wrong count never matches.
            let mut bad = cvs.clone();
            bad[cvs.len() / 2][0] ^= 1;
            assert_ne!(
                root(&bad, len as u64),
                Some(*blake3::hash(bytes).as_bytes())
            );
            assert_eq!(root(&cvs[..cvs.len() - 1], len as u64), None);
        }
        assert_eq!(root(&[[0; 32]], BLOCK as u64), None);
    }
}
