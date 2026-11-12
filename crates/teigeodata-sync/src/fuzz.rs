//! Fuzz and property harnesses (feature `fuzz`, D-072).
//!
//! Each harness takes raw bytes and panics if a property fails. The same functions run under
//! `cargo test` (a seeded random driver in `tests/fuzz.rs`) and under libFuzzer (`fuzz/`).
//!
//! Properties:
//! - [`table`]: `SeekTable::parse` never panics and accepts only canonical tables. A layout
//!   from the input cuts the pack into frames that tile it, never cross a section start and
//!   never exceed 1 MiB; `SeekTable::check` accepts a table, lying or not, exactly when an
//!   independent model does; every plan covers what was wanted and nothing more.
//! - [`asset`]: a pack from the writer, its asset, then one attack: a lying seek table, a
//!   truncated asset, a changed frame byte. A client either refuses it, or gets each frame at
//!   exactly its size and bytes that are the pack or that the pack's hashes refuse.
//! - [`frame`]: the frame decoder never panics and never gives more than its stated length.

#![allow(
    clippy::missing_panics_doc,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test harnesses: a panic is how a property failure is reported, and inputs are bounded"
)]

use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use teigeodata_format::{Encoding, Kind, Pack, Writer};

use crate::Error;
use crate::transport::{
    Decoder, Frames, Layout, MAX_FRAME, MAX_HEAD_FRAME, MIN_FRAME_LEN, SectionSpan, SeekTable,
    compress_bound, encode, encode_table, store,
};

/// A size limit of a harness: `native` normally, `miri` under Miri.
const fn limit(native: usize, miri: usize) -> usize {
    if cfg!(miri) { miri } else { native }
}

/// Reads structured values from fuzz bytes; runs out into zeros.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> u8 {
        let (v, rest) = self.0.split_first().map_or((0, &[][..]), |(v, r)| (*v, r));
        self.0 = rest;
        v
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([0; 4].map(|_| self.u8()))
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { u64::from(self.u32()) % n }
    }
    fn bytes(&mut self, max: usize) -> &'a [u8] {
        let n = usize::from(self.u16()) % (max + 1);
        let (v, rest) = self.0.split_at(n.min(self.0.len()));
        self.0 = rest;
        v
    }
}

/// `SeekTable::parse` on the input itself, then a layout, a table and plans built from it.
pub fn table(data: &[u8]) {
    if let Ok(t) = SeekTable::parse(data) {
        assert_eq!(SeekTable::tail_len(data), Ok(data.len()));
        assert_eq!(t.byte_len(), data.len());
        // Canonical up to what the spec says not to interpret: checksums and unused bits.
        if !t.has_checksums() && data[data.len() - 5].trailing_zeros() >= 2 {
            let mut again = Vec::new();
            encode_table(
                t.entries().map(|e| (e.compressed, e.decompressed)),
                &mut again,
            )
            .unwrap_or_else(|e| panic!("an accepted table does not re-encode: {e}"));
            assert_eq!(again, data, "an accepted table is not canonical");
        }
    }
    let mut c = Cursor(data);
    let Some(layout) = layout(&mut c) else { return };
    check_layout(&layout);
    // A seek table for the layout whose compressed sizes may lie, and an asset length that may.
    let sizes: Vec<u64> = layout.frame_sizes().collect();
    let entries: Vec<(u32, u32)> = sizes
        .iter()
        .map(|&d| {
            let honest = MIN_FRAME_LEN + c.below(compress_bound(d) - MIN_FRAME_LEN + 1);
            let lie = match c.u8() % 8 {
                0 => c.below(MIN_FRAME_LEN),
                1 => compress_bound(d) + 1 + c.below(1 << 16),
                _ => honest,
            };
            (lie.min(u64::from(u32::MAX)) as u32, d as u32)
        })
        .collect();
    let lie_d = c.u8().is_multiple_of(8) && !entries.is_empty();
    let mut entries = entries;
    if lie_d {
        let i = c.below(entries.len() as u64) as usize;
        entries[i].1 = entries[i].1.wrapping_add(1 + u32::from(c.u8() % 64));
    }
    let mut bytes = Vec::new();
    encode_table(entries.iter().copied(), &mut bytes).unwrap_or_else(|e| panic!("{e}"));
    let sum: u64 = entries.iter().map(|e| u64::from(e.0)).sum();
    let asset_len = match c.u8() % 8 {
        0 => sum + bytes.len() as u64 + 1,
        1 => (sum + bytes.len() as u64).saturating_sub(1),
        _ => sum + bytes.len() as u64,
    };
    let model_ok = !lie_d
        && entries.iter().all(|&(cs, d)| {
            u64::from(cs) >= MIN_FRAME_LEN && u64::from(cs) <= compress_bound(u64::from(d))
        })
        && asset_len == sum + bytes.len() as u64;
    let t = SeekTable::parse(&bytes).unwrap_or_else(|e| panic!("own table refused: {e}"));
    match t.check(asset_len, layout) {
        Ok(frames) => {
            assert!(model_ok, "a lying table was accepted");
            check_frames(&frames, &entries, asset_len);
            for _ in 0..4 {
                plan(&frames, &mut c);
            }
        }
        Err(e) => assert!(!model_ok, "an honest table was refused: {e}"),
    }
}

/// A layout from the input: a file of up to 8 MiB, up to 6 sections, gaps and empty sections.
fn layout(c: &mut Cursor<'_>) -> Option<Layout> {
    let head = c.below(MAX_HEAD_FRAME + 64);
    let mut at = head;
    let mut spans = Vec::new();
    for k in 0..u32::from(c.u8() % 7) {
        let len = match c.u8() % 4 {
            0 => 0,
            1 => c.below(4096),
            2 => c.below(3 * MAX_FRAME),
            _ => MAX_FRAME * (1 + c.below(3)) - c.below(2),
        };
        spans.push(SectionSpan {
            kind: Kind(k + 1),
            part: teigeodata_format::Part::BASE,
            offset: at,
            len,
        });
        at += len + c.below(96);
    }
    let file_len = at + c.below(64);
    let head = spans.first().map_or(file_len, |s| s.offset);
    let expect = file_len > 0 && head <= MAX_HEAD_FRAME;
    let l = Layout::new(file_len, spans);
    assert_eq!(
        l.is_ok(),
        expect,
        "Layout::new disagrees with the model: {l:?}"
    );
    l.ok()
}

fn check_layout(l: &Layout) {
    let ranges: Vec<Range<u64>> = l.frame_ranges().collect();
    assert_eq!(ranges.len(), l.frame_count());
    let mut at = 0;
    for r in &ranges {
        assert_eq!(r.start, at, "frames do not tile the pack");
        assert!(r.end > r.start, "an empty frame");
        assert!(r.end - r.start <= MAX_FRAME, "a frame over 1 MiB");
        at = r.end;
    }
    assert_eq!(at, l.file_len());
    if let Some(head) = l.sections().first().map(|s| s.offset).filter(|&h| h > 0) {
        assert!(head <= MAX_HEAD_FRAME);
        assert_eq!(
            ranges.first(),
            Some(&(0..head)),
            "the head is not one frame"
        );
    }
    for s in l.sections() {
        // A section starts a frame (unless it is empty at the end), and its interior cuts fall
        // every 1 MiB from its start, on the 16 KiB block grid.
        if s.offset < l.file_len() {
            assert!(
                ranges.iter().any(|r| r.start == s.offset),
                "a frame crosses a section start"
            );
        }
        for r in &ranges {
            if r.start > s.offset && r.start < s.offset + s.len {
                assert_eq!(
                    (r.start - s.offset) % MAX_FRAME,
                    0,
                    "a cut off the 1 MiB grid"
                );
            }
        }
    }
}

fn check_frames(frames: &Frames, entries: &[(u32, u32)], asset_len: u64) {
    assert_eq!(frames.len(), entries.len());
    let mut at = 0;
    for (i, (f, e)) in frames.iter().zip(entries).enumerate() {
        assert_eq!(Some(&f), frames.frame(i).as_ref());
        assert_eq!(f.compressed, at..at + u64::from(e.0));
        assert_eq!(f.decoded.end - f.decoded.start, u64::from(e.1));
        at = f.compressed.end;
    }
    assert_eq!(at + frames.table_len(), asset_len);
    assert_eq!(frames.frame(frames.len()), None);
}

/// Plans random ranges and checks the fetches against a brute-force model.
fn plan(frames: &Frames, c: &mut Cursor<'_>) {
    let len = frames.layout().file_len();
    let mut wanted = Vec::new();
    for _ in 0..c.u8() % 5 {
        let a = c.below(len + 1);
        let b = c.below(len + 1);
        wanted.push(a.min(b)..a.max(b));
    }
    if c.u8().is_multiple_of(8) {
        let past = Range {
            start: 0,
            end: len + 1,
        };
        assert_eq!(frames.plan(core::slice::from_ref(&past)), Err(Error::Range));
    }
    let fetches = frames.plan(&wanted).unwrap_or_else(|e| panic!("{e}"));
    let need: Vec<bool> = frames
        .iter()
        .map(|f| {
            // An empty range wants no byte, so no frame (`plan` ignores it).
            wanted
                .iter()
                .any(|w| !w.is_empty() && w.start < f.decoded.end && f.decoded.start < w.end)
        })
        .collect();
    let mut got = vec![false; frames.len()];
    let mut prev_end: Option<usize> = None;
    for f in &fetches {
        assert!(!f.frames.is_empty());
        if let Some(p) = prev_end {
            assert!(f.frames.start > p, "fetches overlap, touch or are unsorted");
        }
        prev_end = Some(f.frames.end);
        let first = frames
            .frame(f.frames.start)
            .unwrap_or_else(|| panic!("no frame"));
        let last = frames
            .frame(f.frames.end - 1)
            .unwrap_or_else(|| panic!("no frame"));
        assert_eq!(f.compressed, first.compressed.start..last.compressed.end);
        assert_eq!(f.decoded, first.decoded.start..last.decoded.end);
        for g in f.frames.clone() {
            got[g] = true;
        }
    }
    // Exactly the needed frames, except that a run may not hold a gap: runs are maximal, so
    // fetched equals needed.
    assert_eq!(
        got, need,
        "the plan fetches the wrong frames for {wanted:?}: {fetches:?}"
    );
}

/// A pack from the writer, its transport asset, then an attack and a client's read.
pub fn asset(data: &[u8]) {
    let mut c = Cursor(data);
    // The attack first, so it does not depend on how much input the sections took.
    let (attack, compressed, bit) = (c.u8() % 4, c.u8().is_multiple_of(2), 1_u8 << (c.u8() % 8));
    let (p1, p2, delta) = (
        u64::from(c.u32()),
        u64::from(c.u32()),
        1 + u32::from(c.u8()),
    );
    let mut owned: Vec<(Kind, Vec<u8>)> = Vec::new();
    for k in [Kind::IDMAP, Kind::PLACES, Kind::NAMES, Kind::ARCS] {
        if c.u8().is_multiple_of(3) {
            continue;
        }
        let body = if c.u8().is_multiple_of(16) {
            // A section over 1 MiB, cut into frames; repeated, so it compresses.
            let seed = c.u8();
            let n = limit(MAX_FRAME as usize + 1 + c.below(70_000) as usize, 4097);
            (0..n).map(|i| seed ^ (i / 97) as u8).collect()
        } else {
            c.bytes(limit(3000, 300)).to_vec()
        };
        owned.push((k, body));
    }
    let sections: Vec<(Kind, Encoding, u64, &[u8])> = owned
        .iter()
        .map(|(k, b)| (*k, Encoding::RAW, 1, b.as_slice()))
        .collect();
    let pack = Writer::new("np", "places", "2026.11.0")
        .and_then(|w| w.pack(&sections, None))
        .unwrap_or_else(|e| panic!("the writer refused a fuzz pack: {e}"));
    let asset = encode(&pack, |b| {
        Some(if compressed {
            ruzstd::encoding::compress_to_vec(b, ruzstd::encoding::CompressionLevel::Fastest)
        } else {
            store(b)
        })
    })
    .unwrap_or_else(|e| panic!("the writer refused a valid pack: {e}"));
    let layout = Layout::of(&Pack::open(&pack).unwrap_or_else(|e| panic!("{e}")))
        .unwrap_or_else(|e| panic!("{e}"));
    let table_len = SeekTable::tail_len(&asset).unwrap_or_else(|e| panic!("{e}"));
    let body = asset.len() - table_len;

    let mut bad = asset.clone();
    match attack {
        // A lying table: any byte of it changed.
        0 => bad[body + (p1 % table_len as u64) as usize] ^= bit,
        // Compressed sizes shifted between two frames, the total kept.
        1 => {
            let n = layout.frame_count();
            let (i, j) = ((p1 % n as u64) as usize, (p2 % n as u64) as usize);
            let at = |k: usize| body + 8 + 8 * k;
            let read = |b: &[u8], k: usize| {
                u32::from_le_bytes([b[at(k)], b[at(k) + 1], b[at(k) + 2], b[at(k) + 3]])
            };
            let (ci, cj) = (read(&bad, i), read(&bad, j));
            if i != j && ci > delta {
                bad[at(i)..at(i) + 4].copy_from_slice(&(ci - delta).to_le_bytes());
                bad[at(j)..at(j) + 4].copy_from_slice(&(cj + delta).to_le_bytes());
            }
        }
        // Truncated.
        2 => bad.truncate((p1 % asset.len() as u64) as usize),
        // A frame byte changed.
        _ => bad[(p1 % body as u64) as usize] ^= bit,
    }
    match client(&bad, layout) {
        Ok(out) => {
            assert_eq!(out.len(), pack.len(), "frames decoded past their sizes");
            match attack {
                0 | 1 => assert_eq!(out, pack, "a lying table installed wrong bytes"),
                2 => panic!("a truncated asset was accepted"),
                _ => assert!(
                    out == pack || Pack::open_verified(&out).is_err(),
                    "a changed frame gave a pack its hashes accept"
                ),
            }
        }
        Err(_) => assert!(bad != asset, "the untouched asset was refused"),
    }
}

/// A client's whole-asset read: the seek table from the tail, checked; every frame decoded.
fn client(asset: &[u8], layout: Layout) -> Result<Vec<u8>, Error> {
    let tail = SeekTable::tail_len(asset)?;
    let start = asset
        .len()
        .checked_sub(tail)
        .ok_or(Error::SeekTable("longer than the asset"))?;
    let frames = SeekTable::parse(&asset[start..])?.check(asset.len() as u64, layout)?;
    let mut d = Decoder::new();
    let mut out = Vec::new();
    let mut buf = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let src = &asset[f.compressed.start as usize..f.compressed.end as usize];
        d.frame(i, &f, src, &mut buf)?;
        assert_eq!(buf.len() as u64, f.decoded.end - f.decoded.start);
        out.extend_from_slice(&buf);
    }
    Ok(out)
}

/// The frame decoder on arbitrary bytes and a stated length.
pub fn frame(data: &[u8]) {
    let mut c = Cursor(data);
    let n = match c.u8() % 4 {
        0 => usize::from(c.u16()),
        1 => MAX_FRAME as usize + 1 + usize::from(c.u8()),
        _ => usize::from(c.u8()),
    };
    let src = c.0;
    let mut out = Vec::new();
    match Decoder::new().decode(src, n, &mut out) {
        Ok(()) => assert_eq!(out.len(), n),
        Err(_) => assert!(out.len() <= n),
    }
    // A valid frame of the input itself decodes to it, stored or compressed.
    let mut d = Decoder::new();
    let raw = &data[..data.len().min(limit(4096, 256))];
    for f in [
        store(raw),
        ruzstd::encoding::compress_to_vec(raw, ruzstd::encoding::CompressionLevel::Fastest),
    ] {
        d.decode(&f, raw.len(), &mut out)
            .unwrap_or_else(|e| panic!("a valid frame was refused: {e}"));
        assert_eq!(out, raw);
        if !raw.is_empty() {
            assert!(
                d.decode(&f, raw.len() - 1, &mut out).is_err(),
                "a frame decoded past n"
            );
        }
        assert!(
            d.decode(&f, raw.len() + 1, &mut out).is_err(),
            "a short frame was accepted"
        );
    }
}
