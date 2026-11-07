//! BLOBS (kind 14): cold byte strings (attribution text, long descriptions, licences) in zstd
//! frames, with an optional trained dictionary (B4, D-079). Each frame holds whole blobs, so a
//! blob is one frame decode; a [`Decoder`] keeps its last frame, so neighbours cost a slice.
//!
//! A [column table](teigeodata_core::table); `item_count` is the number of blobs:
//!
//! | ID | Column | Values | |
//! |---:|---|---|---|
//! | 1 | [`FRAME_END`] | per frame: where its bytes end in [`HEAP`]; the first starts after the dictionary | required |
//! | 2 | [`RAW_END`] | per frame: where its decoded bytes end in the blobs' concatenation | required |
//! | 3 | [`BLOB_END`] | per blob: where it ends in the concatenation; it starts where the previous one ends | required |
//! | 4 | [`DICT_END`] | one value: the zstd dictionary is `HEAP[..DICT_END]`; absent, there is none | optional |
//! | 5 | [`HEAP`] | bytes: the dictionary, then every frame, back to back, zero-padded to a multiple of 4 | required |
//!
//! Frames are non-empty, at most [`MAX_FRAME`] decoded bytes, and end on blob ends. When there
//! is a dictionary, every frame uses it. [`Blobs::open`] checks the frame of the section and
//! every column's structure and length in O(columns); [`Blobs::check`] checks every value in
//! O(blobs); [`Decoder::check`] (feature `zstd`) decodes every frame. Reads never panic.
//!
//! Decoding (feature `zstd`) uses `ruzstd`, a pure-Rust decoder whose internal `unsafe` is
//! accepted as a dependency's (D-079). It allocates: a window of at most [`MAX_FRAME`] and the
//! frame's bytes. Output is bounded by the frame's stated length, so a crafted frame cannot
//! expand past it.

use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;

use crate::{Error, Kind, Section, columns};

/// Where each frame ends in the heap.
pub const FRAME_END: u16 = 1;
/// Where each frame's decoded bytes end.
pub const RAW_END: u16 = 2;
/// Where each blob ends.
pub const BLOB_END: u16 = 3;
/// Where the dictionary ends in the heap.
pub const DICT_END: u16 = 4;
/// The dictionary and the frames.
pub const HEAP: u16 = 5;

/// The most decoded bytes in one frame, and so in one blob: 8 MiB. It bounds a decoder's memory.
pub const MAX_FRAME: usize = 8 << 20;
/// The largest dictionary: 1 MiB (trained dictionaries are 16–112 KiB).
pub const MAX_DICT: usize = 1 << 20;

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::BLOBS.0,
        column,
        problem,
    }
}

/// The BLOBS section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Blobs<'a> {
    frame_end: Packed<'a>,
    raw_end: Packed<'a>,
    blob_end: Packed<'a>,
    dict: Option<&'a [u8]>,
    heap: &'a [u8],
}

impl<'a> Blobs<'a> {
    /// Opens the section: the frame, every column's structure and length, and the dictionary's
    /// span, in O(columns + blocks).
    ///
    /// # Errors
    /// [`Error::Payload`] if the section is not a BLOBS column table or a column is missing,
    /// malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let t: Table<'a> = columns::open(section, Kind::BLOBS)?;
        let rows = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the blob count does not fit this platform"))?;
        Self::from_table(&t, rows)
    }

    fn from_table(t: &Table<'a>, rows: usize) -> Result<Self, Error> {
        let frame_end = columns::required(t, Kind::BLOBS, FRAME_END, None)?;
        let raw_end = columns::required(t, Kind::BLOBS, RAW_END, Some(frame_end.len()))?;
        let blob_end = columns::required(t, Kind::BLOBS, BLOB_END, Some(rows))?;
        let dict_end = columns::optional(t, Kind::BLOBS, DICT_END, 1)?;
        let heap = t.bytes(HEAP).map_err(|e| columns::map(Kind::BLOBS, e))?;
        if t.ids().any(|id| id == 0 || id > HEAP) {
            return Err(fail(0, "an unknown column"));
        }
        let dict = match dict_end.and_then(|c| c.get(0)) {
            None => None,
            Some(0) => return Err(fail(DICT_END, "an empty dictionary is written as none")),
            Some(d) if d as usize > MAX_DICT => {
                return Err(fail(DICT_END, "the dictionary is larger than 1 MiB"));
            }
            Some(d) => Some(
                heap.get(..d as usize)
                    .ok_or_else(|| fail(DICT_END, "the dictionary is outside the heap"))?,
            ),
        };
        Ok(Self {
            frame_end,
            raw_end,
            blob_end,
            dict,
            heap,
        })
    }

    /// The number of blobs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.blob_end.len()
    }

    /// Whether there are no blobs.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of frames.
    #[must_use]
    pub const fn frames(&self) -> usize {
        self.frame_end.len()
    }

    /// The dictionary, if the section has one.
    #[must_use]
    pub const fn dictionary(&self) -> Option<&'a [u8]> {
        self.dict
    }

    /// Where blob `i` lies in the blobs' concatenation; `None` past the end. O(1) with the
    /// writer's codings.
    #[must_use]
    pub fn span(&self, i: usize) -> Option<core::ops::Range<usize>> {
        let end = self.blob_end.get(i)? as usize;
        let start = match i.checked_sub(1) {
            Some(p) => self.blob_end.get(p)? as usize,
            None => 0,
        };
        Some(start..end)
    }

    #[cfg(feature = "zstd")]
    /// Frame `f`: its bytes in the heap and its decoded span.
    fn frame(&self, f: usize) -> Result<(&'a [u8], core::ops::Range<usize>), Error> {
        let bad = || fail(FRAME_END, "a frame span is invalid");
        let (end, raw_end) = (self.frame_end.get(f), self.raw_end.get(f));
        let (Some(end), Some(raw_end)) = (end, raw_end) else {
            return Err(fail(FRAME_END, "no such frame"));
        };
        let (start, raw_start) = match f.checked_sub(1) {
            Some(p) => (
                self.frame_end.get(p).ok_or_else(bad)?,
                self.raw_end.get(p).ok_or_else(bad)?,
            ),
            None => (
                self.dict
                    .map_or(0, |d| u32::try_from(d.len()).unwrap_or(u32::MAX)),
                0,
            ),
        };
        if start >= end || raw_start >= raw_end {
            return Err(bad());
        }
        let raw = raw_start as usize..raw_end as usize;
        if raw.len() > MAX_FRAME {
            return Err(fail(RAW_END, "a frame decodes to more than 8 MiB"));
        }
        let bytes = self
            .heap
            .get(start as usize..end as usize)
            .ok_or_else(bad)?;
        Ok((bytes, raw))
    }

    #[cfg(feature = "zstd")]
    /// The frame whose decoded span holds offset `at`: a binary search over [`RAW_END`],
    /// O(log frames).
    fn frame_of(&self, at: usize) -> Result<usize, Error> {
        let (mut lo, mut hi) = (0, self.frames());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let end = self
                .raw_end
                .get(mid)
                .ok_or_else(|| fail(RAW_END, "no such frame"))?;
            if (end as usize) <= at {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo < self.frames() {
            Ok(lo)
        } else {
            Err(fail(BLOB_END, "a blob is past the last frame"))
        }
    }

    /// Checks every value: frames are non-empty, in order, at most [`MAX_FRAME`] decoded
    /// bytes, inside the heap with zero padding after the last; blobs do not go backwards,
    /// end at the last frame's end, and every frame ends on a blob end. O(blobs + frames).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the first column found wrong.
    pub fn check(&self) -> Result<(), Error> {
        let n = self.frames();
        let mut end = self.dict.map_or(0, <[u8]>::len) as u64;
        for e in self.frame_end.values(0..n) {
            if u64::from(e) <= end {
                return Err(fail(FRAME_END, "frames must be non-empty and in order"));
            }
            end = u64::from(e);
        }
        let padding = self
            .heap
            .get(usize::try_from(end).unwrap_or(usize::MAX)..)
            .ok_or_else(|| fail(FRAME_END, "a frame is outside the heap"))?;
        if padding.len() >= 4 || padding.iter().any(|&b| b != 0) {
            return Err(fail(HEAP, "the heap has bytes past the last frame"));
        }
        let mut raw = 0_u64;
        for e in self.raw_end.values(0..n) {
            let e = u64::from(e);
            if e <= raw || e - raw > MAX_FRAME as u64 {
                return Err(fail(
                    RAW_END,
                    "frames must decode to 1 B to 8 MiB, in order",
                ));
            }
            raw = e;
        }
        // Every frame end is a blob end: walk both, in order.
        let mut frames = self.raw_end.values(0..n).peekable();
        let mut prev = 0_u32;
        for b in self.blob_end.values(0..self.len()) {
            if b < prev {
                return Err(fail(BLOB_END, "blobs must not go backwards"));
            }
            while let Some(&f) = frames.peek() {
                if f > b {
                    break;
                }
                if f > prev && f < b {
                    return Err(fail(BLOB_END, "a blob crosses a frame end"));
                }
                frames.next();
            }
            prev = b;
        }
        // Every frame was walked: the last one ends at `raw`.
        if u64::from(prev) != raw {
            return Err(fail(
                BLOB_END,
                "the last blob must end at the last frame's end",
            ));
        }
        Ok(())
    }

    /// A decoder for this section: the dictionary parsed once, then blobs read one by one.
    ///
    /// # Errors
    /// [`Error::Payload`] if the dictionary is not a zstd dictionary.
    #[cfg(feature = "zstd")]
    pub fn decoder(&self) -> Result<Decoder<'a>, Error> {
        Decoder::new(self)
    }
}

/// Reads blobs, keeping the last decoded frame (feature `zstd`).
#[cfg(feature = "zstd")]
pub struct Decoder<'a> {
    blobs: Blobs<'a>,
    zstd: ruzstd::decoding::FrameDecoder,
    dict: Option<u32>,
    frame: Option<usize>,
    buf: alloc::vec::Vec<u8>,
}

#[cfg(feature = "zstd")]
impl core::fmt::Debug for Decoder<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Decoder")
            .field("frame", &self.frame)
            .field("dict", &self.dict)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "zstd")]
impl<'a> Decoder<'a> {
    fn new(blobs: &Blobs<'a>) -> Result<Self, Error> {
        use ruzstd::decoding::{Dictionary, FrameDecoder};
        let mut zstd = FrameDecoder::new();
        zstd.set_max_window_size(MAX_FRAME as u64);
        let dict = match blobs.dict {
            Some(raw) => {
                let d = Dictionary::decode_dict(raw)
                    .map_err(|_| fail(DICT_END, "the dictionary is not a zstd dictionary"))?;
                let id = d.id;
                zstd.add_dict(d)
                    .map_err(|_| fail(DICT_END, "the dictionary is not a zstd dictionary"))?;
                Some(id)
            }
            None => None,
        };
        Ok(Self {
            blobs: *blobs,
            zstd,
            dict,
            frame: None,
            buf: alloc::vec::Vec::new(),
        })
    }

    /// Blob `i`: a slice of its frame, decoded unless it is the last frame read. O(log frames)
    /// to find the frame, then O(frame) to decode it.
    ///
    /// # Errors
    /// [`Error::Payload`] if `i` is past the end, its span is invalid, or its frame does not
    /// decode to exactly its stated length.
    pub fn get(&mut self, i: usize) -> Result<&[u8], Error> {
        let span = self
            .blobs
            .span(i)
            .ok_or_else(|| fail(BLOB_END, "no such blob"))?;
        if span.start > span.end {
            return Err(fail(BLOB_END, "blobs must not go backwards"));
        }
        if span.is_empty() {
            return Ok(&[]);
        }
        let f = self.blobs.frame_of(span.start)?;
        let raw = self.load(f)?;
        // The buffer holds the frame, so a blob past its end is refused here.
        self.buf
            .get(span.start - raw.start..span.end - raw.start)
            .ok_or_else(|| fail(BLOB_END, "a blob crosses a frame end"))
    }

    /// Decodes every frame: each must decode to exactly its stated length and end where its
    /// bytes end. O(decoded bytes).
    ///
    /// # Errors
    /// [`Error::Payload`] for the first frame that does not.
    pub fn check(&mut self) -> Result<(), Error> {
        for f in 0..self.blobs.frames() {
            self.load(f)?;
        }
        Ok(())
    }

    /// Frame `f` decoded into the buffer (unless it is there already); its decoded span.
    fn load(&mut self, f: usize) -> Result<core::ops::Range<usize>, Error> {
        let (bytes, raw) = self.blobs.frame(f)?;
        if self.frame != Some(f) {
            self.frame = None;
            self.decode(bytes, raw.len())?;
            self.frame = Some(f);
        }
        Ok(raw)
    }

    /// Decodes one frame of `n` bytes, in steps, never holding more than its window, one step
    /// and `n` bytes; stops at the first byte past `n`.
    fn decode(&mut self, mut src: &[u8], n: usize) -> Result<(), Error> {
        use ruzstd::decoding::BlockDecodingStrategy;
        use ruzstd::io::Read;
        const STEP: usize = 256 << 10;
        let bad = || fail(HEAP, "a frame does not decode");
        let long = || fail(HEAP, "a frame decodes to more than its stated length");
        self.buf.clear();
        self.buf.resize(n, 0);
        self.zstd.reset(&mut src).map_err(|_| bad())?;
        if let Some(id) = self.dict {
            self.zstd.force_dict(id).map_err(|_| bad())?;
        }
        let mut at = 0;
        loop {
            let finished = self.zstd.is_finished();
            if !finished {
                let before = src.len();
                self.zstd
                    .decode_blocks(&mut src, BlockDecodingStrategy::UptoBytes(STEP))
                    .map_err(|_| bad())?;
                // Every call reads a block header at least; a call that reads nothing and
                // does not finish would loop forever.
                if src.len() == before && !self.zstd.is_finished() {
                    return Err(bad());
                }
            }
            let ready = self.zstd.can_collect();
            let out = self.buf.get_mut(at..at + ready).ok_or_else(long)?;
            let got = self.zstd.read(out).map_err(|_| bad())?;
            at += got;
            if finished && got == 0 {
                break;
            }
        }
        if at != n || !src.is_empty() {
            return Err(fail(HEAP, "a frame's length differs from its span"));
        }
        Ok(())
    }
}

/// Encodes BLOBS (features `write` and `zstd`): `blobs` grouped in order into frames of about
/// `frame_target` decoded bytes (a larger blob gets a frame of its own), each compressed by
/// `compress` with `dict` (the forge uses C zstd at level 19), then every blob read back
/// through [`Decoder`] and compared, so a compressor fault cannot reach a pack.
///
/// # Errors
/// [`Error::Write`] if a blob is over [`MAX_FRAME`], the dictionary is over [`MAX_DICT`] or
/// empty, the section passes 4 GiB, `compress` fails, or a frame does not read back.
#[cfg(all(feature = "write", feature = "zstd"))]
pub fn encode(
    blobs: &[&[u8]],
    dict: Option<&[u8]>,
    frame_target: usize,
    mut compress: impl FnMut(&[u8]) -> Option<alloc::vec::Vec<u8>>,
) -> Result<alloc::vec::Vec<u8>, Error> {
    use alloc::vec::Vec;
    use teigeodata_core::bitpack::encode_direct;
    use teigeodata_core::table::{Codec, encode_with};

    let refuse = Error::Write;
    let big = || refuse("BLOBS passes 4 GiB");
    if dict.is_some_and(|d| d.is_empty() || d.len() > MAX_DICT) {
        return Err(refuse("a BLOBS dictionary must be 1 B to 1 MiB"));
    }
    let mut heap: Vec<u8> = dict.unwrap_or_default().to_vec();
    let (mut frame_end, mut raw_end, mut blob_end) = (Vec::new(), Vec::new(), Vec::new());
    let mut raw: Vec<u8> = Vec::new();
    let mut total = 0_u32;
    let mut flush = |raw: &mut Vec<u8>, heap: &mut Vec<u8>, total: u32| -> Result<(), Error> {
        if raw.is_empty() {
            return Ok(());
        }
        let z = compress(raw).ok_or(refuse("the compressor failed"))?;
        if z.is_empty() {
            return Err(refuse("the compressor wrote an empty frame"));
        }
        heap.extend_from_slice(&z);
        frame_end.push(u32::try_from(heap.len()).map_err(|_| big())?);
        raw_end.push(total);
        raw.clear();
        Ok(())
    };
    for b in blobs {
        if b.len() > MAX_FRAME {
            return Err(refuse("a blob is larger than 8 MiB"));
        }
        if !raw.is_empty() && raw.len() + b.len() > frame_target.min(MAX_FRAME) {
            flush(&mut raw, &mut heap, total)?;
        }
        raw.extend_from_slice(b);
        total = u32::try_from(b.len())
            .ok()
            .and_then(|n| total.checked_add(n))
            .ok_or_else(big)?;
        blob_end.push(total);
    }
    flush(&mut raw, &mut heap, total)?;
    drop(flush);
    heap.resize(heap.len().next_multiple_of(4), 0);
    let mut cols: Vec<(u16, Codec, Vec<u8>)> = alloc::vec![
        (FRAME_END, Codec::Bitpack, encode_direct(&frame_end)),
        (RAW_END, Codec::Bitpack, encode_direct(&raw_end)),
        (BLOB_END, Codec::Bitpack, encode_direct(&blob_end)),
    ];
    if let Some(d) = dict {
        let d = u32::try_from(d.len()).map_err(|_| big())?;
        cols.push((DICT_END, Codec::Bitpack, encode_direct(&[d])));
    }
    cols.push((HEAP, Codec::Bytes, heap));
    let refs: Vec<(u16, Codec, &[u8])> = cols
        .iter()
        .map(|(i, c, b)| (*i, *c, b.as_slice()))
        .collect();
    let body = encode_with(&refs).map_err(|_| big())?;
    // Read every blob back through the reader.
    let back = || refuse("a BLOBS frame does not read back");
    let t = Table::parse(&body).map_err(|_| back())?;
    let view = Blobs::from_table(&t, blobs.len()).map_err(|_| back())?;
    view.check().map_err(|_| back())?;
    let mut d = view.decoder().map_err(|_| back())?;
    for (i, b) in blobs.iter().enumerate() {
        if d.get(i).map_err(|_| back())? != *b {
            return Err(back());
        }
    }
    Ok(body)
}
