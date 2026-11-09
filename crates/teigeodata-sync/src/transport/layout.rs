//! The canonical frame cuts of a pack, and the frame planner.

use alloc::vec::Vec;
use core::ops::Range;

use teigeodata_format::blocks::MIN_COVERED;
use teigeodata_format::component::{Component, Components};
use teigeodata_format::{Kind, Pack};

use super::{MAX_FRAME, MAX_FRAMES, MAX_HEAD_FRAME};
use crate::Error;

/// Where a section lies in the pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionSpan {
    /// The section's kind.
    pub kind: Kind,
    /// Its first byte in the pack.
    pub offset: u64,
    /// Its stored length.
    pub len: u64,
}

/// A pack's layout and its canonical frame cuts:
/// - a cut at 0 and at each section's start;
/// - then every [`MAX_FRAME`] bytes after a section's start, while inside the span up to the
///   next section (or the end of the file).
///
/// Padding after a section belongs to its frame; the footer to the last section's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    file_len: u64,
    /// Frame starts: strictly increasing, the first 0, all below `file_len`.
    starts: Vec<u64>,
    sections: Vec<SectionSpan>,
}

impl Layout {
    /// The layout of an open pack.
    ///
    /// # Errors
    /// As [`Layout::new`].
    pub fn of(pack: &Pack<'_>) -> Result<Self, Error> {
        let base = pack.as_bytes().as_ptr().addr();
        let mut spans = Vec::with_capacity(pack.sections().len());
        for s in pack.sections() {
            let at = s.bytes().as_ptr().addr().checked_sub(base);
            let offset = at
                .and_then(|a| u64::try_from(a).ok())
                .ok_or(Error::Layout("a section is outside its pack"))?;
            let len = u64::try_from(s.bytes().len()).map_err(|_| Error::Layout("too long"))?;
            spans.push(SectionSpan {
                kind: s.kind(),
                offset,
                len,
            });
        }
        let file_len =
            u64::try_from(pack.as_bytes().len()).map_err(|_| Error::Layout("too long"))?;
        Self::new(file_len, spans)
    }

    /// A layout from the file length and the section table, in file order.
    ///
    /// # Errors
    /// [`Error::Layout`] if the file is empty, the sections overlap, run past the file or are
    /// not in file order, the head is over [`MAX_HEAD_FRAME`], or there would be more than
    /// [`MAX_FRAMES`] frames.
    pub fn new(file_len: u64, sections: Vec<SectionSpan>) -> Result<Self, Error> {
        let bad = Error::Layout;
        if file_len == 0 {
            return Err(bad("the pack is empty"));
        }
        let mut end = 0_u64;
        for s in &sections {
            if s.offset < end {
                return Err(bad("sections overlap or are out of file order"));
            }
            end = s
                .offset
                .checked_add(s.len)
                .filter(|&e| e <= file_len)
                .ok_or(bad("a section runs past the file"))?;
        }
        let head = sections.first().map_or(file_len, |s| s.offset);
        if head > MAX_HEAD_FRAME {
            return Err(bad("the head is over 16 KiB"));
        }
        // Count first, so a huge claimed length is refused before anything is allocated.
        let mut section_starts: Vec<u64> = sections.iter().map(|s| s.offset).collect();
        section_starts.dedup();
        let mut count = usize::from(head > 0);
        for (i, &s) in section_starts.iter().enumerate() {
            let e = section_starts.get(i + 1).copied().unwrap_or(file_len);
            let n = (e - s).div_ceil(MAX_FRAME);
            count = usize::try_from(n)
                .ok()
                .and_then(|n| count.checked_add(n))
                .filter(|&c| c <= MAX_FRAMES)
                .ok_or(bad("too many frames"))?;
        }
        let mut starts = Vec::with_capacity(count);
        if head > 0 {
            starts.push(0);
        }
        for (i, &s) in section_starts.iter().enumerate() {
            let e = section_starts.get(i + 1).copied().unwrap_or(file_len);
            let mut at = s;
            while at < e {
                starts.push(at);
                at = at.saturating_add(MAX_FRAME);
            }
        }
        Ok(Self {
            file_len,
            starts,
            sections,
        })
    }

    /// The pack's length.
    #[must_use]
    pub const fn file_len(&self) -> u64 {
        self.file_len
    }

    /// The number of frames.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.starts.len()
    }

    /// Each frame's decoded byte range, in order. They tile the pack.
    pub fn frame_ranges(&self) -> impl ExactSizeIterator<Item = Range<u64>> + '_ {
        (0..self.starts.len()).map(|i| {
            let start = self.starts.get(i).copied().unwrap_or(self.file_len);
            start..self.starts.get(i + 1).copied().unwrap_or(self.file_len)
        })
    }

    /// Each frame's decoded size, in order.
    pub fn frame_sizes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        self.frame_ranges().map(|r| r.end - r.start)
    }

    /// The sections, in file order.
    #[must_use]
    pub fn sections(&self) -> &[SectionSpan] {
        &self.sections
    }

    /// The frames that hold `range` of the pack (empty for an empty range).
    fn frames_of(&self, range: &Range<u64>) -> Range<usize> {
        if range.start >= range.end {
            return 0..0;
        }
        let first = self.starts.partition_point(|&s| s <= range.start);
        let last = self.starts.partition_point(|&s| s < range.end);
        first.saturating_sub(1)..last
    }
}

/// One frame of a checked asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Its byte range in the asset.
    pub compressed: Range<u64>,
    /// The byte range of the pack it decodes to.
    pub decoded: Range<u64>,
}

/// One range request: a run of adjacent frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetch {
    /// The frames, as indexes into the seek table.
    pub frames: Range<usize>,
    /// The byte range to request from the asset.
    pub compressed: Range<u64>,
    /// The byte range of the pack they decode to.
    pub decoded: Range<u64>,
}

/// A seek table checked against its asset and pack ([`SeekTable::check`](super::SeekTable::check)):
/// every frame's ranges in the asset and in the pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frames {
    layout: Layout,
    /// Frame starts in the asset, then the end of the last frame.
    compressed: Vec<u64>,
    table_len: u64,
}

impl Frames {
    pub(super) const fn new(layout: Layout, compressed: Vec<u64>, table_len: u64) -> Self {
        Self {
            layout,
            compressed,
            table_len,
        }
    }

    /// The pack layout the table was checked against.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The number of frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.layout.frame_count()
    }

    /// Whether there are no frames (never, for a checked table of a non-empty pack).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The length of the seek-table frame at the end of the asset.
    #[must_use]
    pub const fn table_len(&self) -> u64 {
        self.table_len
    }

    /// Frame `i`.
    #[must_use]
    pub fn frame(&self, i: usize) -> Option<Frame> {
        let start = *self.layout.starts.get(i)?;
        let end = self.layout.starts.get(i + 1).copied();
        Some(Frame {
            compressed: *self.compressed.get(i)?..*self.compressed.get(i + 1)?,
            decoded: start..end.unwrap_or(self.layout.file_len),
        })
    }

    /// Every frame, in order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Frame> + '_ {
        self.layout
            .frame_ranges()
            .zip(self.compressed.windows(2))
            .map(|(decoded, c)| Frame {
                compressed: c.first().copied().unwrap_or(0)..c.get(1).copied().unwrap_or(0),
                decoded,
            })
    }

    /// The fetches that cover `wanted` byte ranges of the pack: each wanted byte lies in a
    /// fetched frame, each fetched frame holds a wanted byte, and adjacent frames are merged
    /// into one request (a host may refuse multi-range requests). Sorted, never overlapping or
    /// touching. Empty ranges are ignored.
    ///
    /// # Errors
    /// [`Error::Range`] if a range ends before it starts or runs past the pack.
    pub fn plan(&self, wanted: &[Range<u64>]) -> Result<Vec<Fetch>, Error> {
        let mut runs: Vec<Range<usize>> = Vec::new();
        for r in wanted {
            if r.start > r.end || r.end > self.layout.file_len {
                return Err(Error::Range);
            }
            let f = self.layout.frames_of(r);
            if !f.is_empty() {
                runs.push(f);
            }
        }
        runs.sort_unstable_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(runs.len());
        for r in runs {
            match merged.last_mut() {
                Some(m) if r.start <= m.end => m.end = m.end.max(r.end),
                _ => merged.push(r),
            }
        }
        Ok(merged.into_iter().filter_map(|f| self.fetch(f)).collect())
    }

    /// The fetches for the sections of `kinds` (kinds the pack lacks are ignored), as
    /// [`Frames::plan`].
    ///
    /// # Errors
    /// As [`Frames::plan`]; never for a checked layout.
    pub fn plan_sections(&self, kinds: &[Kind]) -> Result<Vec<Fetch>, Error> {
        let wanted: Vec<Range<u64>> = self
            .layout
            .sections
            .iter()
            .filter(|s| kinds.contains(&s.kind))
            .map(|s| s.offset..s.offset + s.len)
            .collect();
        self.plan(&wanted)
    }

    /// The fetches that install `components` (D-133, D-136): the head frame, which every
    /// install holds; the sections of `components` and everything they need
    /// ([`Components::closed`]); and `BLOCK_HASHES` when one of those sections is
    /// [`MIN_COVERED`] or more, whose blocks it verifies. Adding a component later is the plan
    /// for the new set less the frames already held.
    ///
    /// # Errors
    /// As [`Frames::plan`]; never for a checked layout.
    pub fn plan_components(&self, components: Components) -> Result<Vec<Fetch>, Error> {
        let set = components.closed();
        let chosen = |k: Kind| Component::of(k).is_some_and(|c| set.contains(c));
        let blocked = self
            .layout
            .sections
            .iter()
            .any(|s| chosen(s.kind) && s.len >= MIN_COVERED);
        let mut wanted: Vec<Range<u64>> = Vec::with_capacity(self.layout.sections.len() + 1);
        // The head: the first frame, up to the first section.
        let head_end = self
            .layout
            .starts
            .get(1)
            .copied()
            .unwrap_or(self.layout.file_len);
        wanted.push(0..head_end);
        wanted.extend(
            self.layout
                .sections
                .iter()
                .filter(|s| chosen(s.kind) || (blocked && s.kind == Kind::BLOCK_HASHES))
                .map(|s| s.offset..s.offset + s.len),
        );
        self.plan(&wanted)
    }

    fn fetch(&self, frames: Range<usize>) -> Option<Fetch> {
        let first = self.frame(frames.start)?;
        let last = self.frame(frames.end.checked_sub(1)?)?;
        Some(Fetch {
            frames,
            compressed: first.compressed.start..last.compressed.end,
            decoded: first.decoded.start..last.decoded.end,
        })
    }
}
