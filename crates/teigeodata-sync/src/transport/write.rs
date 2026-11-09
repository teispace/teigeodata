//! The transport writer (feature `write`): release tooling and tests.
//!
//! The compressor is the caller's, so C zstd stays out of this crate (the forge uses level 19).
//! Every frame is decoded again with [`Decoder`], the client's decoder, and compared with the
//! pack before it is written; [`encode`] then reads the whole asset back through the client's
//! path ([`verify`]). A compressor fault cannot reach a published asset.

use alloc::vec::Vec;

use teigeodata_format::Pack;

use super::{Decoder, Layout, MIN_FRAME_LEN, SeekTable, ZSTD_MAGIC, compress_bound, encode_table};
use crate::Error;

/// What [`write`] wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Written {
    /// The number of zstd frames (the seek table not counted).
    pub frames: usize,
    /// The asset's length.
    pub asset_len: u64,
    /// The length of the seek-table frame at its end.
    pub table_len: u64,
}

fn at(pack: &[u8], r: core::ops::Range<u64>) -> Option<&[u8]> {
    pack.get(usize::try_from(r.start).ok()?..usize::try_from(r.end).ok()?)
}

/// Writes the transport asset of `pack` to `sink`, one frame at a time, with bounded memory:
/// each frame of the [`Layout`] compressed by `compress` (one zstd frame per call), decoded
/// again and compared, then the seek table.
///
/// # Errors
/// [`Error::Pack`] if `pack` is not a valid pack (its section hashes are checked);
/// [`Error::Write`] if `compress` fails or returns anything but one zstd frame that decodes to
/// its input, or the table fails the reader's checks; any error of `sink`.
pub fn write(
    pack: &[u8],
    mut compress: impl FnMut(&[u8]) -> Option<Vec<u8>>,
    mut sink: impl FnMut(&[u8]) -> Result<(), Error>,
) -> Result<Written, Error> {
    let layout = Layout::of(&Pack::open_verified(pack)?)?;
    let mut decoder = Decoder::new();
    let mut back = Vec::new();
    let mut entries = Vec::with_capacity(layout.frame_count());
    let mut asset_len = 0_u64;
    for r in layout.frame_ranges() {
        let data = at(pack, r).ok_or(Error::Write("a frame is outside the pack"))?;
        let d = u64::try_from(data.len()).map_err(|_| Error::Write("a frame is too large"))?;
        let c = compress(data).ok_or(Error::Write("the compressor failed"))?;
        let c_len = u64::try_from(c.len()).map_err(|_| Error::Write("a frame is too large"))?;
        if c.get(..4) != Some(&ZSTD_MAGIC.to_le_bytes()[..])
            || c_len < MIN_FRAME_LEN
            || c_len > compress_bound(d)
        {
            return Err(Error::Write("the compressor did not return one zstd frame"));
        }
        decoder
            .decode(&c, data.len(), &mut back)
            .map_err(|_| Error::Write("a frame does not decode"))?;
        if back != data {
            return Err(Error::Write("a frame does not decode to its bytes"));
        }
        sink(&c)?;
        let entry = (u32::try_from(c_len), u32::try_from(d));
        let (Ok(c32), Ok(d32)) = entry else {
            return Err(Error::Write("a frame is too large"));
        };
        entries.push((c32, d32));
        asset_len += c_len;
    }
    let mut table = Vec::new();
    encode_table(entries.iter().copied(), &mut table)?;
    sink(&table)?;
    let table_len = u64::try_from(table.len()).map_err(|_| Error::Write("too many frames"))?;
    asset_len += table_len;
    // The reader must accept what was written.
    let frames = SeekTable::parse(&table)
        .and_then(|t| t.check(asset_len, layout))
        .map_err(|_| Error::Write("the seek table fails the reader's checks"))?;
    Ok(Written {
        frames: frames.len(),
        asset_len,
        table_len,
    })
}

/// The transport asset of `pack`, in memory: [`write`], then [`verify`] of the result.
///
/// # Errors
/// As [`write`]; [`Error::Write`] if the asset does not read back as `pack`.
pub fn encode(
    pack: &[u8],
    compress: impl FnMut(&[u8]) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write(pack, compress, |b| {
        out.extend_from_slice(b);
        Ok(())
    })?;
    verify(&out, pack).map_err(|_| Error::Write("the asset does not read back as the pack"))?;
    Ok(out)
}

/// Reads `asset` back as a client does (the seek table from its tail, checked against the
/// layout of `pack`, then every frame decoded) and compares the result with `pack`, byte for
/// byte.
///
/// # Errors
/// The first problem: [`Error::Pack`], [`Error::SeekTable`], [`Error::Frame`], or
/// [`Error::Write`] if the decoded bytes differ from `pack`.
pub fn verify(asset: &[u8], pack: &[u8]) -> Result<(), Error> {
    let layout = Layout::of(&Pack::open(pack)?)?;
    let tail = SeekTable::tail_len(asset)?;
    let table = asset
        .len()
        .checked_sub(tail)
        .and_then(|s| asset.get(s..))
        .ok_or(Error::SeekTable("longer than the asset"))?;
    let asset_len = u64::try_from(asset.len()).map_err(|_| Error::SeekTable("too long"))?;
    let frames = SeekTable::parse(table)?.check(asset_len, layout)?;
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let src = at(asset, f.compressed.clone()).ok_or(Error::Frame {
            index: i,
            problem: "outside the asset",
        })?;
        decoder.frame(i, &f, src, &mut out)?;
        if at(pack, f.decoded).is_none_or(|want| want != out.as_slice()) {
            return Err(Error::Write(
                "a frame decodes to bytes that differ from the pack",
            ));
        }
    }
    Ok(())
}

/// `data` as one zstd frame of raw (stored) blocks: no compression, but a valid frame that any
/// zstd decoder reads. For a section that does not compress, tests, and fuzzing.
#[must_use]
pub fn store(data: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 128 << 10;
    let n = data.len();
    let mut out = Vec::with_capacity(n + 16 + 3 * (n / BLOCK + 1));
    out.extend_from_slice(&ZSTD_MAGIC.to_le_bytes());
    // Frame header descriptor: single segment (bit 5), so the content size is the window.
    if let Ok(v) = u8::try_from(n) {
        out.push(0b0010_0000);
        out.push(v);
    } else if let Some(v) = n.checked_sub(256).and_then(|v| u16::try_from(v).ok()) {
        out.push(0b0110_0000);
        out.extend_from_slice(&v.to_le_bytes());
    } else if let Ok(v) = u32::try_from(n) {
        out.push(0b1010_0000);
        out.extend_from_slice(&v.to_le_bytes());
    } else {
        out.push(0b1110_0000);
        out.extend_from_slice(&(n as u64).to_le_bytes());
    }
    let mut chunks = data.chunks(BLOCK).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0]); // the last block, raw, empty
    }
    while let Some(c) = chunks.next() {
        let last = u32::from(chunks.peek().is_none());
        // Raw block type 0; at most 128 KiB, so the size fits its 21 bits.
        let header = last | (u32::try_from(c.len()).unwrap_or(0) << 3);
        out.extend_from_slice(header.to_le_bytes().get(..3).unwrap_or_default());
        out.extend_from_slice(c);
    }
    out
}
