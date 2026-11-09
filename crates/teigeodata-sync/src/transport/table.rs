//! The seek table: a skippable frame at the end of the asset, in the zstd seekable format
//! v0.1.0 (`contrib/seekable_format` of facebook/zstd):
//!
//! ```text
//! Skippable_Magic_Number u32 (0x184D2A5E) | Frame_Size u32 | entries | footer
//! entry:  Compressed_Size u32 | Decompressed_Size u32 | [Checksum u32]
//! footer: Number_Of_Frames u32 | Seek_Table_Descriptor u8 | Seekable_Magic_Number u32 (0x8F92EAB1)
//! ```
//!
//! Descriptor bit 7 is `Checksum_Flag`; bits 6–2 are reserved and must be 0; bits 1–0 are unused
//! and not interpreted. We write no checksums (BLAKE3 covers every block), and read tables with
//! or without them.

use alloc::vec::Vec;

use super::layout::{Frames, Layout};
use super::{
    FOOTER_LEN, MAX_FRAMES, MIN_FRAME_LEN, SEEKABLE_MAGIC, SKIPPABLE_MAGIC, compress_bound,
};
use crate::Error;

const CHECKSUM_FLAG: u8 = 0x80;
const RESERVED_BITS: u8 = 0x7C;
/// The skippable frame's magic and size fields.
const FRAME_HEADER_LEN: usize = 8;

/// A parsed seek table, borrowing its bytes. Parsing checks only its own structure;
/// [`SeekTable::check`] checks it against the asset and the pack.
#[derive(Clone, Copy, Debug)]
pub struct SeekTable<'a> {
    entries: &'a [u8],
    stride: usize,
    count: usize,
}

/// One seek-table entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The frame's compressed size.
    pub compressed: u32,
    /// The frame's decompressed size.
    pub decompressed: u32,
    /// The low 32 bits of the XXH64 of the decompressed bytes, when the table has checksums.
    pub checksum: Option<u32>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let v = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([
        *v.first()?,
        *v.get(1)?,
        *v.get(2)?,
        *v.get(3)?,
    ]))
}

/// Count and descriptor from the 9-byte footer at the end of `bytes`.
fn footer(bytes: &[u8]) -> Result<(usize, usize), Error> {
    let bad = Error::SeekTable;
    let at = bytes
        .len()
        .checked_sub(FOOTER_LEN)
        .ok_or(bad("shorter than its footer"))?;
    if u32_at(bytes, at + 5) != Some(SEEKABLE_MAGIC) {
        return Err(bad("the asset does not end with the seekable magic number"));
    }
    let descriptor = *bytes.get(at + 4).ok_or(bad("shorter than its footer"))?;
    if descriptor & RESERVED_BITS != 0 {
        return Err(bad("reserved descriptor bits are set"));
    }
    let count = u32_at(bytes, at).ok_or(bad("shorter than its footer"))?;
    let count = usize::try_from(count).map_err(|_| bad("too many frames"))?;
    if count > MAX_FRAMES {
        return Err(bad("too many frames"));
    }
    let stride = if descriptor & CHECKSUM_FLAG == 0 {
        8
    } else {
        12
    };
    Ok((count, stride))
}

impl<'a> SeekTable<'a> {
    /// The length of the whole seek-table frame, from the last 9 bytes of `asset_tail` (any
    /// suffix of the asset at least 9 bytes long). A client that does not know it from the
    /// catalog fetches the footer first, then this many bytes from the end.
    ///
    /// # Errors
    /// [`Error::SeekTable`] if the footer is malformed.
    pub fn tail_len(asset_tail: &[u8]) -> Result<usize, Error> {
        let (count, stride) = footer(asset_tail)?;
        // Bounded by MAX_FRAMES, so this cannot overflow.
        Ok(FRAME_HEADER_LEN + count * stride + FOOTER_LEN)
    }

    /// Parses `bytes`, which must be exactly the seek-table frame (the last
    /// [`SeekTable::tail_len`] bytes of the asset).
    ///
    /// # Errors
    /// [`Error::SeekTable`] if the magic numbers, the frame size, the descriptor or the frame
    /// count are wrong, or the length does not match them.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let bad = Error::SeekTable;
        let (count, stride) = footer(bytes)?;
        if Self::tail_len(bytes)? != bytes.len() {
            return Err(bad("its length does not match its frame count"));
        }
        if u32_at(bytes, 0) != Some(SKIPPABLE_MAGIC) {
            return Err(bad(
                "it is not in a skippable frame with the seek-table magic",
            ));
        }
        let size = u32_at(bytes, 4).ok_or(bad("truncated"))?;
        if usize::try_from(size).ok() != bytes.len().checked_sub(FRAME_HEADER_LEN) {
            return Err(bad("the skippable frame size does not match its length"));
        }
        let entries = bytes
            .get(FRAME_HEADER_LEN..bytes.len() - FOOTER_LEN)
            .ok_or(bad("truncated"))?;
        Ok(Self {
            entries,
            stride,
            count,
        })
    }

    /// The number of frames.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether the table lists no frames.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The length of the whole seek-table frame, in bytes.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        // Bounded by MAX_FRAMES, so this cannot overflow.
        FRAME_HEADER_LEN + self.count * self.stride + FOOTER_LEN
    }

    /// Whether the entries carry checksums.
    #[must_use]
    pub const fn has_checksums(&self) -> bool {
        self.stride == 12
    }

    /// Entry `i`.
    #[must_use]
    pub fn entry(&self, i: usize) -> Option<Entry> {
        let at = i.checked_mul(self.stride)?;
        Some(Entry {
            compressed: u32_at(self.entries, at)?,
            decompressed: u32_at(self.entries, at + 4)?,
            checksum: if self.stride == 12 {
                Some(u32_at(self.entries, at + 8)?)
            } else {
                None
            },
        })
    }

    /// Every entry, in frame order.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = Entry> + '_ {
        let checksums = self.stride == 12;
        self.entries.chunks_exact(self.stride).map(move |e| Entry {
            compressed: u32_at(e, 0).unwrap_or(0),
            decompressed: u32_at(e, 4).unwrap_or(0),
            checksum: if checksums { u32_at(e, 8) } else { None },
        })
    }

    /// Checks the table against the asset's length and the pack's `layout` (from its signed
    /// head), and gives the frames' byte ranges.
    ///
    /// The table is accepted only if:
    /// - it lists exactly the layout's frames, each with the decoded size of its cut, so no
    ///   frame crosses a section boundary or exceeds [`MAX_FRAME`](super::MAX_FRAME);
    /// - every compressed size is between [`MIN_FRAME_LEN`] and
    ///   [`compress_bound`] of its decoded size;
    /// - the compressed sizes and the table fill `asset_len` exactly.
    ///
    /// # Errors
    /// [`Error::SeekTable`] or [`Error::Frame`], naming the first disagreement.
    pub fn check(&self, asset_len: u64, layout: Layout) -> Result<Frames, Error> {
        if self.count != layout.frame_count() {
            return Err(Error::SeekTable(
                "its frame count differs from the pack's layout",
            ));
        }
        let mut starts = Vec::new();
        starts
            .try_reserve_exact(self.count + 1)
            .map_err(|_| Error::SeekTable("too many frames"))?;
        let mut at = 0_u64;
        starts.push(at);
        for (index, (e, cut)) in self.entries().zip(layout.frame_sizes()).enumerate() {
            let fail = |problem| Error::Frame { index, problem };
            let (c, d) = (u64::from(e.compressed), u64::from(e.decompressed));
            if d != cut {
                return Err(fail(
                    "its decoded size is not the pack's cut (it crosses a section or is too large)",
                ));
            }
            if c < MIN_FRAME_LEN || c > compress_bound(d) {
                return Err(fail(
                    "its compressed size is impossible for its decoded size",
                ));
            }
            at = at
                .checked_add(c)
                .ok_or(fail("the asset passes 2^64 bytes"))?;
            starts.push(at);
        }
        let table_len = u64::try_from(self.byte_len()).map_err(|_| Error::SeekTable("too long"))?;
        if at.checked_add(table_len) != Some(asset_len) {
            return Err(Error::SeekTable(
                "the frames and the table do not fill the asset exactly",
            ));
        }
        Ok(Frames::new(layout, starts, table_len))
    }
}

/// Appends a seek table without checksums for frames of `(compressed, decompressed)` sizes.
///
/// # Errors
/// [`Error::Write`] if there are more than [`MAX_FRAMES`] frames.
pub fn encode_table(
    entries: impl ExactSizeIterator<Item = (u32, u32)>,
    out: &mut Vec<u8>,
) -> Result<(), Error> {
    let count = entries.len();
    if count > MAX_FRAMES {
        return Err(Error::Write("too many frames"));
    }
    let n = u32::try_from(count).map_err(|_| Error::Write("too many frames"))?;
    let size =
        u32::try_from(count * 8 + FOOTER_LEN).map_err(|_| Error::Write("too many frames"))?;
    out.extend_from_slice(&SKIPPABLE_MAGIC.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    for (c, d) in entries {
        out.extend_from_slice(&c.to_le_bytes());
        out.extend_from_slice(&d.to_le_bytes());
    }
    out.extend_from_slice(&n.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&SEEKABLE_MAGIC.to_le_bytes());
    Ok(())
}
