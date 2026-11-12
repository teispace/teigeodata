//! The sans-I/O installer (D-167): a pack from its transport asset, by range requests, every
//! byte checked against the signed catalog before it is written.
//!
//! The caller does the I/O: it fetches the byte ranges the installer asks for, in any way (an
//! HTTP client, a background download, a file), and writes the bytes the installer hands back
//! at their offsets in a staging file of the pack's length (sparse where a component is not
//! installed). The installer never holds more than one fetch and one decoded frame.
//!
//! 1. [`Install::new`] from the verified catalog entry, the asset's length (the TUF target's)
//!    and the components to install. [`Install::first`] gives the two first requests: the seek
//!    table at the end and the head frame at the start.
//! 2. [`Install::start`] takes both. The head is checked against the catalog (head hash, length,
//!    address, component, release) and against itself ([`PackHead::parse`]); the table against
//!    the head. It writes the head frame and, when the last section is not installed, the
//!    footer, and returns the fetches: `BLOCK_HASHES` first, then the sections in file order.
//! 3. [`Install::feed`] takes one fetch's bytes (any run of wanted frames still to do). Each frame is decoded with its output capped
//!    at its size, then checked: a section under 1 MiB whole against its hash, a larger one
//!    block by block against its proven block hashes, padding as zeros, the footer as the
//!    head's. Only then is it written. `BLOCK_HASHES` itself is written once it is whole and
//!    proven.
//! 4. [`Install::finish`] once every wanted frame is done.
//!
//! Resume (D-169): progress is per frame. After `start`, [`Install::staged`] offers frames an
//! earlier attempt left in staging; each is checked as a fetched one and kept if it passes, and
//! [`Install::remaining`] gives the fetches for the rest. `BLOCK_HASHES` is never kept, so it is
//! fed first again and covered frames are checked against the fresh list. No resume record is
//! needed: the signed hashes are the record, and a torn write is just a frame still to do.
//!
//! Trust: the catalog entry is signed (TUF, D-159), it gives the head hash, the head's table
//! gives every section's BLAKE3, and `BLOCK_HASHES` gives every block's chaining value under
//! that hash. Nothing the host sends is written before it is bound to that chain; a lying host
//! or table costs a request, never a wrong byte.

use alloc::vec::Vec;
use core::ops::Range;

use teigeodata_format::blocks::{BLOCK, MIN_COVERED, leaf, proven_cvs};
use teigeodata_format::component::{Parts, Selection};
use teigeodata_format::{Encoding, Kind, PackHead, Part, SectionEntry};

use crate::Error;
use crate::catalog::Entry;
use crate::transport::{Decoder, Fetch, Frame, Frames, SeekTable};

/// A section of the pack, owned (the head is parsed once, then let go).
#[derive(Clone, Debug)]
struct Section {
    kind: Kind,
    part: Part,
    encoding: Encoding,
    item_count: u64,
    decoded_length: u64,
    hash: [u8; 32],
    offset: u64,
    length: u64,
    /// For a section of 1 MiB or more being installed: its proven block hashes.
    cvs: Option<Vec<[u8; 32]>>,
}

impl Section {
    fn entry(&self) -> SectionEntry<'_> {
        SectionEntry {
            kind: self.kind,
            part: self.part,
            encoding: self.encoding,
            item_count: self.item_count,
            decoded_length: self.decoded_length,
            hash: &self.hash,
            offset: self.offset,
            length: self.length,
        }
    }

    const fn end(&self) -> u64 {
        self.offset + self.length
    }
}

/// What the installer knows once it has the head.
#[derive(Debug)]
struct Planned {
    frames: Frames,
    sections: Vec<Section>,
    footer: [u8; teigeodata_format::FOOTER_SIZE],
    pack_len: u64,
    fetches: Vec<Fetch>,
    /// Per frame: whether the plan wants it, and whether it is checked and staged.
    wanted: Vec<bool>,
    done: Vec<bool>,
    /// `BLOCK_HASHES`'s frames (empty when the plan has none). They are fed in one fetch and
    /// count as done only once the list is proven, so a refused list leaves no state behind.
    list_frames: Range<usize>,
    /// `BLOCK_HASHES`, gathered within that one fetch when it spans several frames.
    list: Vec<u8>,
    /// Whether the block hashes the plan needs are proven.
    proven: bool,
    /// Bytes fed and written so far.
    fetched: u64,
    written: u64,
    kept: u64,
}

/// One install of one pack: see the [module](self).
#[derive(Debug)]
pub struct Install {
    asset_len: u64,
    pack_len: u64,
    head_hash: [u8; 16],
    seek_table: u64,
    head_wire: u64,
    head_disk: u64,
    address: alloc::string::String,
    component: alloc::string::String,
    release: alloc::string::String,
    set: Selection,
    decoder: Decoder,
    out: Vec<u8>,
    planned: Option<Planned>,
}

/// What an install fetched and wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Installed {
    /// Bytes requested from the host, the table and the head frame included.
    pub fetched: u64,
    /// Bytes written to the staging file.
    pub written: u64,
    /// Decoded bytes an earlier attempt left in staging that checked and were kept (resume).
    pub kept: u64,
    /// Decoded bytes taken from the release being replaced, checked against this release's
    /// signed hashes before they were written (D-174); set by the driver, 0 here.
    pub reused: u64,
    /// The pack's length: the staging file's, once cut to it.
    pub pack_len: u64,
    /// The parts installed, each language resolved to the part that holds it in this pack
    /// (D-251).
    pub parts: Parts,
}

const fn fail(problem: &'static str) -> Error {
    Error::Install(problem)
}

fn len_of(b: &[u8]) -> u64 {
    b.len() as u64
}

impl Install {
    /// An install of `set` (its components closed over what each needs, and the parts it
    /// holds, D-251) of the pack `entry` names, whose transport asset is `asset_len` bytes (the
    /// length its TUF target gives). A [`Components`](teigeodata_format::component::Components)
    /// installs every part.
    #[must_use]
    pub fn new(entry: &Entry, asset_len: u64, set: impl Into<Selection>) -> Self {
        Self {
            asset_len,
            pack_len: entry.pack_length,
            head_hash: entry.head,
            seek_table: entry.seek_table,
            head_wire: entry.head_frame.wire,
            head_disk: entry.head_frame.disk,
            address: entry.address.clone(),
            component: entry.component.clone(),
            release: entry.release.clone(),
            set: set.into().closed(),
            decoder: Decoder::new(),
            out: Vec::new(),
            planned: None,
        }
    }

    /// The two first requests, which may run at once: the seek table, then the head frame.
    ///
    /// # Errors
    /// [`Error::Install`] if the entry's sizes do not fit the asset.
    pub fn first(&self) -> Result<[Range<u64>; 2], Error> {
        let table = self
            .asset_len
            .checked_sub(self.seek_table)
            .filter(|&s| s >= self.head_wire)
            .ok_or(fail(
                "the seek table and the head frame do not fit the asset",
            ))?;
        Ok([table..self.asset_len, 0..self.head_wire])
    }

    /// Takes the seek table and the head frame (the bytes of [`Install::first`]'s ranges),
    /// checks them, writes the head frame (and the footer when the last section is not
    /// installed) with `write(offset, bytes)`, and returns the fetches to make, in order:
    /// `BLOCK_HASHES` first when the plan needs it, then the sections in file order.
    ///
    /// # Errors
    /// [`Error::Install`] if the asset disagrees with its catalog entry, [`Error::Pack`] for a
    /// head the format refuses, the seek table's and frames' errors, or `write`'s.
    pub fn start<E: From<Error>>(
        &mut self,
        table: &[u8],
        head_frame: &[u8],
        mut write: impl FnMut(u64, &[u8]) -> Result<(), E>,
    ) -> Result<&[Fetch], E> {
        if self.planned.is_some() {
            return Err(fail("started twice").into());
        }
        let mut planned = self.plan(table, head_frame)?;
        write(0, &self.out)?;
        planned.written = len_of(&self.out);
        if !planned.sections.is_empty() && !ends(&planned.fetches, self.pack_len) {
            write(self.pack_len - len_of(&planned.footer), &planned.footer)?;
            planned.written += len_of(&planned.footer);
        }
        Ok(&self.planned.insert(planned).fetches)
    }

    fn plan(&mut self, table: &[u8], head_frame: &[u8]) -> Result<Planned, Error> {
        if len_of(table) != self.seek_table || len_of(head_frame) != self.head_wire {
            return Err(fail("a first request did not return its length"));
        }
        let table = SeekTable::parse(table)?;
        let (range, n) = table.head_frame()?;
        if range.end != self.head_wire || n != self.head_disk {
            return Err(fail("the head frame is not the catalog's"));
        }
        let n = usize::try_from(n).map_err(|_| fail("too large"))?;
        self.decoder
            .decode(head_frame, n, &mut self.out)
            .map_err(|problem| Error::Frame { index: 0, problem })?;
        let head = PackHead::parse(&self.out, self.pack_len)?;
        let m = head.manifest();
        if head.digest().get(..16) != Some(&self.head_hash[..]) {
            return Err(fail("the head hash is not the catalog's"));
        }
        if (m.address(), m.component(), m.release())
            != (&*self.address, &*self.component, &*self.release)
        {
            return Err(fail("the pack is not the one the catalog names"));
        }
        // The languages asked for, each as the part that holds it in this pack (D-251).
        self.set.parts = self.set.parts.resolve(|p| {
            head.sections()
                .any(|e| e.kind == Kind::NAMES && e.part == p)
        });
        let frames = table.frames_from_head(self.asset_len, &self.out)?;
        let fetches = frames.plan_install(self.set)?;
        let mut sections: Vec<Section> = head
            .sections()
            .map(|e| Section {
                kind: e.kind,
                part: e.part,
                encoding: e.encoding,
                item_count: e.item_count,
                decoded_length: e.decoded_length,
                hash: *e.hash,
                offset: e.offset,
                length: e.length,
                cvs: None,
            })
            .collect();
        // An empty section has no frame: check its hash now.
        for s in sections.iter().filter(|s| s.length == 0) {
            if blake3::hash(&[]).as_bytes() != &s.hash {
                return Err(teigeodata_format::Error::SectionHash { kind: s.kind.0 }.into());
            }
        }
        let needs_list = fetches.iter().any(|f| {
            sections
                .iter()
                .any(|s| s.length >= MIN_COVERED && s.kind != Kind::BLOCK_HASHES && overlaps(f, s))
        });
        sections.sort_unstable_by_key(|s| (s.offset, s.length));
        let list_frames = match sections
            .iter()
            .find(|s| s.kind == Kind::BLOCK_HASHES && s.length > 0)
        {
            Some(s) if needs_list => {
                let span = s.offset..s.end();
                let mut idx = (0..frames.len()).filter(|&i| {
                    frames
                        .frame(i)
                        .is_some_and(|f| span.contains(&f.decoded.start))
                });
                let a = idx.next().unwrap_or(0);
                a..idx.next_back().map_or(a + 1, |b| b + 1)
            }
            _ => 0..0,
        };
        let mut wanted = alloc::vec![false; frames.len()];
        for f in &fetches {
            for w in wanted.get_mut(f.frames.clone()).into_iter().flatten() {
                *w = true;
            }
        }
        Ok(Planned {
            done: alloc::vec![false; frames.len()],
            wanted,
            fetched: self.seek_table + self.head_wire,
            written: 0,
            kept: 0,
            footer: head.footer(),
            pack_len: self.pack_len,
            frames,
            sections,
            fetches,
            list_frames,
            list: Vec::new(),
            proven: !needs_list,
        })
    }

    /// Takes the bytes of `fetch` (one of [`Install::start`]'s, or of [`Install::remaining`]'s
    /// on a resume), checks each of its frames and writes it with `write(offset, bytes)`.
    /// `BLOCK_HASHES`, when the plan has it, must be fed first; the others in any order.
    ///
    /// # Errors
    /// [`Error::Install`] for a fetch that is not a run of wanted frames still to do, bytes of
    /// the wrong length, or a covered section before its block hashes; [`Error::Frame`] for a
    /// frame that does not decode to its size; [`Error::Pack`] (`SectionHash`) for bytes the
    /// hashes refuse; or `write`'s.
    pub fn feed<E: From<Error>>(
        &mut self,
        fetch: &Fetch,
        bytes: &[u8],
        mut write: impl FnMut(u64, &[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let p = self.planned.as_mut().ok_or(fail("fed before start"))?;
        let first = p.frames.frame(fetch.frames.start);
        let last = fetch
            .frames
            .end
            .checked_sub(1)
            .and_then(|l| p.frames.frame(l));
        let (Some(first), Some(last)) = (first, last) else {
            return Err(fail("not a fetch of this install").into());
        };
        if fetch.compressed != (first.compressed.start..last.compressed.end)
            || fetch.decoded != (first.decoded.start..last.decoded.end)
        {
            return Err(fail("not a fetch of this install").into());
        }
        let todo = |i: usize| p.wanted.get(i) == Some(&true) && p.done.get(i) == Some(&false);
        if !fetch.frames.clone().all(todo) {
            return Err(fail("a frame not wanted, or fed twice").into());
        }
        let lf = &p.list_frames;
        let touches = fetch.frames.start < lf.end && lf.start < fetch.frames.end;
        if touches && !(fetch.frames.start <= lf.start && lf.end <= fetch.frames.end) {
            return Err(fail("the block hashes are fed in one fetch").into());
        }
        p.list.clear();
        if len_of(bytes) != fetch.compressed.end - fetch.compressed.start {
            return Err(fail("a fetch did not return its length").into());
        }
        for i in fetch.frames.clone() {
            let frame = p.frames.frame(i).ok_or(fail("no such frame"))?;
            let at = |r: Range<u64>| {
                usize::try_from(r.start - fetch.compressed.start)
                    .ok()
                    .zip(usize::try_from(r.end - fetch.compressed.start).ok())
            };
            let (a, b) = at(frame.compressed.clone()).ok_or(fail("too large"))?;
            let src = bytes.get(a..b).ok_or(fail("a frame outside its fetch"))?;
            self.decoder.frame(i, &frame, src, &mut self.out)?;
            check_frame(p, i, frame.decoded.start, &self.out, &mut write)?;
            p.fetched += frame.compressed.end - frame.compressed.start;
        }
        Ok(())
    }

    /// Resume (D-169): frame `index`'s bytes as an earlier attempt left them in the staging
    /// file (`decoded`, its decoded range of the pack). They are checked as a fetched frame is,
    /// and when they pass the frame counts as done and is not fetched again; a torn or stale
    /// frame stays to do. A frame of a covered section can only be checked once `BLOCK_HASHES`
    /// is fed; `BLOCK_HASHES` itself is never kept (it is fetched again: 32 bytes per 16 KiB it
    /// covers, 0.2%), so a stale list cannot pass a stale frame.
    ///
    /// # Errors
    /// [`Error::Install`] before start, or for a frame not wanted or already done.
    pub fn staged(&mut self, index: usize, decoded: &[u8]) -> Result<bool, Error> {
        let p = self.planned.as_mut().ok_or(fail("staged before start"))?;
        if p.wanted.get(index) != Some(&true) || p.done.get(index) != Some(&false) {
            return Err(fail("a frame not wanted, or already done"));
        }
        let frame = p.frames.frame(index).ok_or(fail("no such frame"))?;
        if p.list_frames.contains(&index)
            || len_of(decoded) != frame.decoded.end - frame.decoded.start
        {
            return Ok(false);
        }
        let mut none = |_: u64, _: &[u8]| Ok::<(), Error>(());
        let written = p.written;
        let ok = check_frame(p, index, frame.decoded.start, decoded, &mut none).is_ok();
        p.written = written;
        if ok {
            p.kept += len_of(decoded);
        }
        Ok(ok)
    }

    /// Whether frame `index` is wanted and still to do (neither fed nor kept from staging).
    #[must_use]
    pub fn todo(&self, index: usize) -> bool {
        self.planned.as_ref().is_some_and(|p| {
            p.wanted.get(index) == Some(&true) && p.done.get(index) == Some(&false)
        })
    }

    /// Whether the block hashes the plan needs are proven (at once when it needs none).
    #[must_use]
    pub fn proven(&self) -> bool {
        self.planned.as_ref().is_some_and(|p| p.proven)
    }

    /// Frame `index`'s ranges, once started.
    #[must_use]
    pub fn frame(&self, index: usize) -> Option<Frame> {
        self.planned.as_ref()?.frames.frame(index)
    }

    /// `fetch` as one fetch per frame, so a driver that streams a response feeds each frame as
    /// soon as its bytes are in, and a cut connection loses at most the frame in flight. The
    /// block-hash list stays one fetch: it is fed whole.
    #[must_use]
    pub fn frames_of(&self, fetch: &Fetch) -> Vec<Fetch> {
        let Some(p) = self.planned.as_ref() else {
            return alloc::vec![fetch.clone()];
        };
        let lf = &p.list_frames;
        if fetch.frames.start < lf.end && lf.start < fetch.frames.end {
            return alloc::vec![fetch.clone()];
        }
        fetch
            .frames
            .clone()
            .filter_map(|i| {
                let f = p.frames.frame(i)?;
                Some(Fetch {
                    frames: i..i + 1,
                    compressed: f.compressed,
                    decoded: f.decoded,
                })
            })
            .collect()
    }

    /// The fetches still to make: runs of wanted frames not yet done, `BLOCK_HASHES`'s first.
    /// After [`Install::start`] it is the plan; after [`Install::staged`] the rest of it.
    #[must_use]
    pub fn remaining(&self) -> Vec<Fetch> {
        let Some(p) = self.planned.as_ref() else {
            return Vec::new();
        };
        let in_list = |i: usize| p.list_frames.contains(&i);
        let todo = |i: &usize| p.wanted.get(*i) == Some(&true) && p.done.get(*i) == Some(&false);
        let mut runs: Vec<(bool, Range<usize>)> = Vec::new();
        for i in (0..p.done.len()).filter(todo) {
            let l = in_list(i);
            match runs.last_mut() {
                Some((rl, r)) if r.end == i && *rl == l => r.end = i + 1,
                _ => runs.push((l, i..i + 1)),
            }
        }
        runs.sort_by_key(|(l, r)| (!l, r.start));
        runs.into_iter()
            .filter_map(|(_, r)| {
                let head = p.frames.frame(r.start)?;
                let tail = p.frames.frame(r.end - 1)?;
                Some(Fetch {
                    frames: r,
                    compressed: head.compressed.start..tail.compressed.end,
                    decoded: head.decoded.start..tail.decoded.end,
                })
            })
            .collect()
    }

    /// The end of the install: every wanted frame done.
    ///
    /// # Errors
    /// [`Error::Install`] if a frame is still to do or the install never started.
    pub fn finish(&self) -> Result<Installed, Error> {
        let p = self.planned.as_ref().ok_or(fail("never started"))?;
        let all = p.wanted.iter().zip(&p.done).all(|(w, d)| !w || *d);
        if !all || !p.proven {
            return Err(fail("a frame was not fed"));
        }
        Ok(Installed {
            fetched: p.fetched,
            written: p.written,
            kept: p.kept,
            reused: 0,
            pack_len: self.pack_len,
            parts: self.set.parts,
        })
    }

    /// The fetches, once started.
    #[must_use]
    pub fn fetches(&self) -> &[Fetch] {
        self.planned.as_ref().map_or(&[], |p| &p.fetches)
    }
}

/// Whether a fetch holds the pack's last frame, and so its footer.
fn ends(fetches: &[Fetch], pack_len: u64) -> bool {
    fetches.iter().any(|f| f.decoded.end == pack_len)
}

fn overlaps(f: &Fetch, s: &Section) -> bool {
    s.length > 0 && f.decoded.start < s.end() && s.offset < f.decoded.end
}

/// Checks one decoded frame starting at pack offset `at` and writes it.
fn check_frame<E: From<Error>>(
    p: &mut Planned,
    index: usize,
    at: u64,
    bytes: &[u8],
    write: &mut impl FnMut(u64, &[u8]) -> Result<(), E>,
) -> Result<(), E> {
    check_and_write(p, at, bytes, write)?;
    if !p.list_frames.contains(&index) {
        if let Some(d) = p.done.get_mut(index) {
            *d = true;
        }
    } else if p.proven {
        for d in p.done.get_mut(p.list_frames.clone()).into_iter().flatten() {
            *d = true;
        }
    }
    Ok(())
}

fn check_and_write<E: From<Error>>(
    p: &mut Planned,
    at: u64,
    bytes: &[u8],
    write: &mut impl FnMut(u64, &[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let end = at + len_of(bytes);
    // The section this frame belongs to: the last non-empty one starting at or before it.
    let i = p
        .sections
        .iter()
        .rposition(|s| s.length > 0 && s.offset <= at)
        .ok_or(fail("a frame before every section"))?;
    let s = p.sections.get(i).ok_or(fail("no such section"))?;
    let (kind, off, s_end) = (s.kind, s.offset, s.end());
    // Past the section: zero padding, then in the pack's last frame the footer.
    let data_end = s_end.min(end);
    let footer_start = p.pack_len - len_of(&p.footer);
    let cut = |x: u64| usize::try_from(x - at).map_err(|_| fail("too large"));
    let (data, rest) = bytes.split_at(cut(data_end)?);
    let (pad, foot) = rest.split_at(cut(footer_start.clamp(data_end, end))? - data.len());
    if pad.iter().any(|&b| b != 0) || !(foot.is_empty() || foot == p.footer) {
        return Err(fail("padding or the footer is not the head's").into());
    }
    let tail = rest;
    let hash_err = || Error::Pack(teigeodata_format::Error::SectionHash { kind: kind.0 });
    if kind == Kind::BLOCK_HASHES {
        // Gathered until whole, then proven and written at once.
        p.list.extend_from_slice(data);
        if data_end < s_end {
            return Ok(());
        }
        let list_entry = p.sections.get(i).ok_or(fail("no such section"))?.entry();
        let list = core::mem::take(&mut p.list);
        let mut proven: Vec<(usize, Vec<[u8; 32]>)> = Vec::new();
        for (j, s) in p.sections.iter().enumerate() {
            if let Some(cvs) = proven_cvs(&list_entry, &list, &s.entry()).map_err(Error::Pack)? {
                proven.push((j, cvs.to_vec()));
            }
        }
        if proven.is_empty() && blake3::hash(&list).as_bytes() != list_entry.hash {
            return Err(hash_err().into());
        }
        for (j, cvs) in proven {
            if let Some(s) = p.sections.get_mut(j) {
                s.cvs = Some(cvs);
            }
        }
        p.proven = true;
        let start = s_end - len_of(&list);
        write(start, &list)?;
        write(start + len_of(&list), tail)?;
        p.written += len_of(&list) + len_of(tail);
        return Ok(());
    }
    match &s.cvs {
        Some(cvs) => {
            let first =
                usize::try_from((at - off) / BLOCK as u64).map_err(|_| fail("too large"))?;
            for (j, block) in data.chunks(BLOCK).enumerate() {
                let n = first + j;
                if cvs.get(n) != Some(&leaf(n as u64, block)) {
                    return Err(hash_err().into());
                }
            }
        }
        None if s.length >= MIN_COVERED => {
            return Err(fail("a large section before its block hashes").into());
        }
        None => {
            if at != off || data_end != s_end || blake3::hash(data).as_bytes() != &s.hash {
                return Err(hash_err().into());
            }
        }
    }
    write(at, bytes)?;
    p.written += len_of(bytes);
    Ok(())
}
