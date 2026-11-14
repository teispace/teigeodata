//! Moving the bytes (D-168): a [`Transport`] makes one ranged request, and [`install`] drives the
//! sans-I/O [`Install`] over one into a staging file.
//!
//! A transport is small on purpose: platform apps bring their own (`URLSession`, `OkHttp`, `fetch`)
//! behind the same trait or drive [`Install`] directly. [`Ureq`] (feature `http`) is the
//! default for desktop and servers. Every response is held to the request: status 206, a
//! `Content-Range` naming exactly the requested bytes and the asset's length, at most those
//! bytes read, and one `ETag` for the whole install, so a host that ignores `Range` or swaps the
//! asset midway is refused, not trusted (the GitHub asset host ignores `If-Range`).
//!
//! A platform whose downloads run without our code (iOS background `URLSession`, Android UIDT
//! jobs, `fetch` into OPFS) installs in passes (D-184): over [`Downloads`], the bytes it has
//! fetched so far, the installer answers [`FetchError::Needs`] with every range still missing;
//! the platform downloads them and runs it again. Every byte is checked as for any transport.
//!
//! An update ([`update_from`], D-174) reads the release it replaces: every frame of the new
//! pack whose sections are unchanged (same kind, hash and length) is put together from the old
//! file, checked against the new release's signed hashes like a fetched frame, and only then
//! written; the rest is fetched. A wrong or damaged old file costs bytes, never correctness.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;
use std::io::{Read, Seek, SeekFrom, Write};

use teigeodata_format::component::Selection;
use teigeodata_format::{FOOTER_SIZE, PackHead};

use crate::Error;
use crate::catalog::Entry;
use crate::install::{Install, Installed};

/// What a response said about the asset, besides its bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Got {
    /// The asset's `ETag`, if the host sent one.
    pub etag: Option<String>,
    /// The asset's whole length, from `Content-Range`.
    pub total: u64,
}

/// The two requests sync makes: a ranged GET for pack assets, and a whole-file GET for the
/// catalog's metadata ([`crate::refresh`], D-178).
pub trait Transport {
    /// Fetches exactly the bytes `range` of the asset at `url` into `out` (cleared first).
    ///
    /// # Errors
    /// A [`FetchError`]: the request failed, or the response was not exactly `range`.
    fn get(&mut self, url: &str, range: Range<u64>, out: &mut Vec<u8>) -> Result<Got, FetchError>;

    /// [`Transport::get`], handing the body to `sink` as it arrives, in order, so the installer
    /// checks and writes each frame as soon as it is in and a cut connection loses at most the
    /// frame in flight. The default fetches the whole range into `buf` and hands it over once;
    /// a transport that can stream overrides it, with the same checks as `get`.
    ///
    /// # Errors
    /// As [`Transport::get`], and `sink`'s.
    fn get_streamed(
        &mut self,
        url: &str,
        range: Range<u64>,
        buf: &mut Vec<u8>,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), FetchError>,
    ) -> Result<Got, FetchError> {
        let got = self.get(url, range, buf)?;
        sink(buf)?;
        Ok(got)
    }

    /// Fetches the whole file at `url` into `out` (cleared first): status 200, at most `max`
    /// bytes after any `Content-Encoding` is decoded (the cap holds against a compression
    /// bomb). Metadata hosts serve gzip, and a transport should accept it.
    ///
    /// # Errors
    /// [`FetchError::Status`] for any other status (404 and 403 end a root walk);
    /// [`FetchError::Response`] for more than `max` bytes; [`FetchError::Transport`] when the
    /// request failed.
    fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError>;

    /// The work about to start expects about `bytes` from the host (the catalog's wire size),
    /// for a transport that reports progress. The default ignores it.
    fn expect(&mut self, bytes: u64) {
        let _ = bytes;
    }
}

/// Why fetching or installing failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum FetchError {
    /// The host answered with this status instead of 206.
    Status(u16),
    /// The response is not the bytes asked for: it says why.
    Response(&'static str),
    /// The request failed (connection, TLS, timeout): the transport's message.
    Transport(String),
    /// The staging file could not be written.
    Io(std::io::Error),
    /// The install refused the bytes.
    Install(Error),
    /// The catalog names an older release than the one installed: a rollback (sync gate 1).
    Older {
        /// The installed release.
        installed: String,
    },
    /// The transport does not hold these byte ranges of the asset yet: a platform downloads
    /// them, then installs again (D-184). Ranges are in file order, adjacent ones merged.
    Needs(Vec<Range<u64>>),
    /// The work was cancelled (a watched transport, D-274).
    Cancelled,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Status(s) => write!(f, "the host answered {s}, not 206 Partial Content"),
            Self::Response(p) => write!(f, "the host's response was refused: {p}"),
            Self::Transport(m) => write!(f, "the request failed: {m}"),
            Self::Io(e) => write!(f, "the staging file: {e}"),
            Self::Install(e) => write!(f, "{e}"),
            Self::Older { installed } => {
                write!(
                    f,
                    "the catalog names a release older than the installed {installed}"
                )
            }
            Self::Needs(r) => {
                let bytes: u64 = r.iter().map(|r| r.end - r.start).sum();
                write!(
                    f,
                    "{} byte ranges ({bytes} bytes) still to download",
                    r.len()
                )
            }
        }
    }
}

impl core::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Install(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Error> for FetchError {
    fn from(e: Error) -> Self {
        Self::Install(e)
    }
}

impl From<std::io::Error> for FetchError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Parses `Content-Range: bytes a-b/total` and checks it names exactly `range`; the total.
///
/// # Errors
/// [`FetchError::Response`] if it is missing, malformed or names other bytes.
pub fn content_range(value: Option<&str>, range: &Range<u64>) -> Result<u64, FetchError> {
    let bad = FetchError::Response("Content-Range is not the range asked for");
    let v = value.ok_or(FetchError::Response("no Content-Range"))?;
    let (span, total) = v
        .trim()
        .strip_prefix("bytes ")
        .and_then(|r| r.split_once('/'))
        .ok_or(FetchError::Response("a malformed Content-Range"))?;
    let (a, b) = span
        .split_once('-')
        .ok_or(FetchError::Response("a malformed Content-Range"))?;
    let num = |s: &str| {
        s.parse::<u64>()
            .map_err(|_| FetchError::Response("a malformed Content-Range"))
    };
    let (a, b, total) = (num(a)?, num(b)?, num(total)?);
    if range.is_empty()
        || a != range.start
        || b.checked_add(1) != Some(range.end)
        || range.end > total
    {
        return Err(bad);
    }
    Ok(total)
}

/// Installs `set` of the pack `entry` names from its asset at `url` (`asset_len` bytes, the TUF
/// target's length) over `transport`, into `staging`, then flushes it. Where nothing is
/// installed the file is sparse. The caller then cuts the file to [`Installed::pack_len`] (an old
/// file may be longer) and calls [`std::fs::File::sync_all`] before renaming it into place.
///
/// Resume (D-169): frames an earlier attempt left in `staging` are read back and checked against
/// the signed hashes like fetched ones; those that pass are kept and only the rest is fetched,
/// so no resume record is needed and a torn, stale or foreign file costs only its bad frames.
/// An empty file is a fresh install. The seek table and head are always fetched again, and one
/// `ETag` holds within an attempt (the asset host ignores `If-Range`, so a changed asset is
/// refused, never mixed).
///
/// # Errors
/// The first [`FetchError`]: a request, a response that is not exactly its range, an asset whose
/// length or `ETag` changes, the install's checks, or the file.
pub fn install<T: Transport, F: Read + Write + Seek>(
    transport: &mut T,
    url: &str,
    entry: &Entry,
    asset_len: u64,
    set: impl Into<Selection>,
    staging: &mut F,
) -> Result<Installed, FetchError> {
    run(transport, url, entry, asset_len, set.into(), staging, None)
}

/// A file the driver reads an earlier release from (D-174).
pub trait Previous: Read + Seek {}

impl<T: Read + Seek> Previous for T {}

/// The asset an install fetches: its path under each mirror, its catalog entry and its length
/// (the TUF target's).
#[derive(Clone, Copy, Debug)]
pub struct Asset<'a> {
    /// The path under each mirror's base.
    pub path: &'a str,
    /// The catalog entry.
    pub entry: &'a Entry,
    /// The asset's length.
    pub len: u64,
}

fn run<T: Transport, F: Read + Write + Seek>(
    transport: &mut T,
    url: &str,
    entry: &Entry,
    asset_len: u64,
    set: Selection,
    staging: &mut F,
    previous: Option<&mut (dyn Previous + '_)>,
) -> Result<Installed, FetchError> {
    let mut inst = Install::new(entry, asset_len, set);
    let mut same = SameAsset::new(asset_len);
    let mut get = |transport: &mut T, range: Range<u64>, out: &mut Vec<u8>| {
        let got = transport.get(url, range, out)?;
        same.check(got)
    };
    let [t, h] = inst.first()?;
    let (mut table, mut head) = (Vec::new(), Vec::new());
    // Both at once when a platform must download them (D-184).
    let mut needs = Needs::default();
    needs.take(get(transport, t, &mut table))?;
    needs.take(get(transport, h, &mut head))?;
    needs.done()?;
    let mut buf = Vec::new();
    // Taken before start writes the head: what the last attempt left.
    let old_len = staging.seek(SeekFrom::End(0))?;
    let write = |f: &mut F, at: u64, b: &[u8]| -> Result<(), FetchError> {
        f.seek(SeekFrom::Start(at))?;
        f.write_all(b)?;
        Ok(())
    };
    let plan = inst
        .start(&table, &head, |at, b| write(staging, at, b))?
        .to_vec();
    let mut reused = 0;
    if old_len > 0 || previous.is_some() {
        // BLOCK_HASHES first (the plan lists it first), always fetched; then keep what checks.
        while !inst.proven() {
            let Some(f) = inst.remaining().into_iter().next() else {
                break;
            };
            get(transport, f.compressed.clone(), &mut buf)?;
            inst.feed(&f, &buf, |at, b| write(staging, at, b))?;
        }
        let frames: Vec<usize> = plan.iter().flat_map(|f| f.frames.clone()).collect();
        if old_len > 0 {
            keep(&mut inst, staging, &frames, old_len, &mut buf)?;
        }
        if let Some(old) = previous {
            reused = reuse(
                &mut inst,
                staging,
                old,
                entry.pack_length,
                &frames,
                &mut buf,
            )?;
        }
    }
    // Each response is streamed and each frame fed as soon as its bytes are in: a cut loses at
    // most the frame in flight, and the next attempt keeps the rest (resume). A frame not yet
    // downloaded is listed, and the rest still fed: what a platform holds is written and kept
    // for the next pass (D-184).
    let mut needs = Needs::default();
    let mut acc: Vec<u8> = Vec::new();
    for f in inst.remaining() {
        let parts = inst.frames_of(&f);
        let mut next = 0;
        acc.clear();
        let mut feed = |chunk: &[u8]| -> Result<(), FetchError> {
            acc.extend_from_slice(chunk);
            let mut used = 0;
            while let Some(p) = parts.get(next) {
                let n = usize::try_from(p.compressed.end - p.compressed.start)
                    .map_err(|_| too_large())?;
                let Some(bytes) = acc.get(used..used + n) else {
                    break;
                };
                inst.feed(p, bytes, |at, b| write(staging, at, b))?;
                used += n;
                next += 1;
            }
            acc.drain(..used);
            Ok(())
        };
        let got = transport.get_streamed(url, f.compressed.clone(), &mut buf, &mut feed);
        if needs.take(got.and_then(|g| same.check(g)))? && next != parts.len() {
            return Err(FetchError::Response("the body is not the range's length"));
        }
    }
    staging.flush()?;
    needs.done()?;
    let mut done = inst.finish()?;
    done.kept -= reused;
    done.reused = reused;
    Ok(done)
}

/// Holds every response of one install to the same asset: the catalog's length, and one `ETag`
/// (or none) for all of them.
struct SameAsset {
    len: u64,
    seen: bool,
    etag: Option<String>,
}

impl SameAsset {
    const fn new(len: u64) -> Self {
        Self {
            len,
            seen: false,
            etag: None,
        }
    }

    fn check(&mut self, got: Got) -> Result<(), FetchError> {
        if got.total != self.len {
            return Err(FetchError::Response(
                "the asset's length is not the catalog's",
            ));
        }
        if !self.seen {
            self.seen = true;
            self.etag = got.etag;
            return Ok(());
        }
        if self.etag == got.etag {
            Ok(())
        } else {
            Err(FetchError::Response(
                "the asset changed on the host during the install",
            ))
        }
    }
}

/// The ranges a pass found missing, merged in file order.
#[derive(Default)]
struct Needs(Vec<Range<u64>>);

impl Needs {
    /// `Ok(true)` for bytes fetched, `Ok(false)` for ranges noted as needed, or the error.
    fn take<T>(&mut self, r: Result<T, FetchError>) -> Result<bool, FetchError> {
        match r {
            Ok(_) => Ok(true),
            Err(FetchError::Needs(more)) => {
                self.0.extend(more);
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// The ranges in file order, adjacent or overlapping ones merged (the plan lists
    /// `BLOCK_HASHES` first, though it lies near the end): fewer, larger downloads.
    fn done(mut self) -> Result<(), FetchError> {
        if self.0.is_empty() {
            return Ok(());
        }
        self.0.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<u64>> = Vec::with_capacity(self.0.len());
        for r in self.0 {
            match merged.last_mut() {
                Some(last) if last.end >= r.start => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        Err(FetchError::Needs(merged))
    }
}

/// The head of a pack of `len` bytes read from `f`: a prefix from the start, doubled until it
/// holds the head; `None` when even the whole file does not, or it is not a pack.
pub(crate) fn read_head<R: Read + Seek + ?Sized>(
    f: &mut R,
    len: u64,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut want: u64 = 16 * 1024;
    loop {
        let n = want.min(len);
        let Ok(size) = usize::try_from(n) else {
            return Ok(None);
        };
        let mut head = alloc::vec![0_u8; size];
        f.seek(SeekFrom::Start(0))?;
        f.read_exact(&mut head)?;
        match PackHead::parse(&head, len) {
            Err(teigeodata_format::Error::Truncated) if n < len => want = want.saturating_mul(2),
            Ok(_) => return Ok(Some(head)),
            Err(_) => return Ok(None),
        }
    }
}

/// Offers each of `frames` still to do whose bytes the old release holds (D-174): a frame is
/// put together from the old file's unchanged sections (same kind, hash and length), zero
/// padding between sections, and the new footer, checked by [`Install::staged`], and written
/// only when it passes. A frame touching the head or a changed section is left to fetch.
/// Returns the bytes reused.
fn reuse<F: Read + Write + Seek>(
    inst: &mut Install,
    staging: &mut F,
    old: &mut (dyn Previous + '_),
    pack_len: u64,
    frames: &[usize],
    buf: &mut Vec<u8>,
) -> Result<u64, FetchError> {
    // The new head, as start wrote it (frame 0); the old one, from the old file.
    let Some(head_end) = inst.frame(0).map(|f| f.decoded.end) else {
        return Ok(0);
    };
    // Exactly the head frame: the staging file may hold nothing past it yet.
    let mut new_head = alloc::vec![0_u8; usize::try_from(head_end).map_err(|_| too_large())?];
    staging.seek(SeekFrom::Start(0))?;
    staging.read_exact(&mut new_head)?;
    let old_len = old.seek(SeekFrom::End(0))?;
    let Some(old_head) = read_head(old, old_len)? else {
        return Ok(0);
    };
    let (Ok(new), Ok(prev)) = (
        PackHead::parse(&new_head, pack_len),
        PackHead::parse(&old_head, old_len),
    ) else {
        return Ok(0);
    };
    // Each new section: its range, and where the old file holds the same bytes.
    let spans: Vec<(Range<u64>, Option<u64>)> = new
        .sections()
        .map(|s| {
            let same = prev.part(s.kind, s.part).filter(|o| {
                o.hash == s.hash && o.length == s.length && o.offset + o.length <= old_len
            });
            (s.offset..s.offset + s.length, same.map(|o| o.offset))
        })
        .collect();
    let body = spans.first().map_or(head_end, |(r, _)| r.start);
    let footer_at = pack_len.saturating_sub(FOOTER_SIZE as u64);
    let footer = new.footer();
    let mut reused = 0;
    'frames: for &i in frames {
        let Some(d) = inst.frame(i).map(|f| f.decoded) else {
            continue;
        };
        if !inst.todo(i) || d.start < body {
            continue;
        }
        buf.clear();
        buf.resize(
            usize::try_from(d.end - d.start).map_err(|_| too_large())?,
            0,
        );
        for (r, from) in &spans {
            let (a, b) = (r.start.max(d.start), r.end.min(d.end));
            if a >= b {
                continue;
            }
            let Some(o) = from else {
                continue 'frames;
            };
            let at = usize::try_from(a - d.start).map_err(|_| too_large())?;
            let to = usize::try_from(b - d.start).map_err(|_| too_large())?;
            old.seek(SeekFrom::Start(o + (a - r.start)))?;
            old.read_exact(buf.get_mut(at..to).ok_or_else(too_large)?)?;
        }
        if d.end > footer_at {
            let a = footer_at.max(d.start);
            let skip = usize::try_from(a - footer_at).map_err(|_| too_large())?;
            let at = usize::try_from(a - d.start).map_err(|_| too_large())?;
            let part = footer.get(skip..).ok_or_else(too_large)?;
            buf.get_mut(at..at + part.len())
                .ok_or_else(too_large)?
                .copy_from_slice(part);
        }
        if inst.staged(i, buf)? {
            staging.seek(SeekFrom::Start(d.start))?;
            staging.write_all(buf)?;
            reused += d.end - d.start;
        }
    }
    Ok(reused)
}

/// The bytes of one asset a platform has downloaded, as files in one directory (D-184): each
/// file is named `<start>-<end>` (decimal, `end` exclusive) and holds exactly those bytes of the
/// asset. A [`Transport`] over them for [`install_from`] and [`update_from`]: a range the files
/// hold is served from them; a range they do not is [`FetchError::Needs`], which the installer
/// collects. Bytes are checked by the installer like any host's, so a file need not be trusted;
/// one whose length is not its range's is refused (a download that ignored `Range`).
#[derive(Clone, Debug)]
pub struct Downloads {
    dir: std::path::PathBuf,
    total: u64,
    parts: Vec<Range<u64>>,
}

impl Downloads {
    /// The files in `dir` (none if it does not exist) of an asset of `total` bytes.
    ///
    /// # Errors
    /// The file system's.
    pub fn open(dir: impl Into<std::path::PathBuf>, total: u64) -> std::io::Result<Self> {
        let dir = dir.into();
        let mut parts = Vec::new();
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for e in entries {
                    let e = e?;
                    if let Some(r) = e.file_name().to_str().and_then(Self::parse)
                        && r.end <= total
                    {
                        parts.push(r);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        parts.sort_by_key(|r| (r.start, r.end));
        Ok(Self { dir, total, parts })
    }

    /// The file name that holds `range`.
    #[must_use]
    pub fn name(range: &Range<u64>) -> String {
        alloc::format!("{}-{}", range.start, range.end)
    }

    fn parse(name: &str) -> Option<Range<u64>> {
        let (a, b) = name.split_once('-')?;
        let (a, b) = (a.parse().ok()?, b.parse().ok()?);
        (a < b).then_some(a..b)
    }

    /// The downloaded ranges, in file order.
    #[must_use]
    pub fn parts(&self) -> &[Range<u64>] {
        &self.parts
    }
}

impl Transport for Downloads {
    fn get(&mut self, _url: &str, range: Range<u64>, out: &mut Vec<u8>) -> Result<Got, FetchError> {
        out.clear();
        let Some(part) = self
            .parts
            .iter()
            .find(|p| p.start <= range.start && range.end <= p.end)
        else {
            return Err(FetchError::Needs(alloc::vec![range]));
        };
        let mut f = std::fs::File::open(self.dir.join(Self::name(part)))?;
        if f.metadata()?.len() != part.end - part.start {
            return Err(FetchError::Response(
                "a downloaded file is not the range it is named for",
            ));
        }
        f.seek(SeekFrom::Start(range.start - part.start))?;
        out.resize(
            usize::try_from(range.end - range.start).map_err(|_| too_large())?,
            0,
        );
        f.read_exact(out)?;
        Ok(Got {
            etag: None,
            total: self.total,
        })
    }

    fn fetch(&mut self, _url: &str, _max: u64, _out: &mut Vec<u8>) -> Result<(), FetchError> {
        Err(FetchError::Transport(
            "metadata is not fetched from downloaded parts".into(),
        ))
    }
}

/// One mirror: a base URL, and how it has done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mirror {
    /// The base URL; an asset's URL is the base followed by its path.
    pub base: String,
    /// Attempts that failed on the network or the protocol (a request, a status, a response
    /// that is not its range).
    pub failures: u32,
    /// Attempts whose bytes the install refused: the mirror served something other than the
    /// signed file.
    pub refused: u32,
}

/// Mirrors in the caller's order (D-170). Every mirror serves the same files at the same paths;
/// since every byte is checked against the signed hashes, a mirror can deny service but never
/// change data. [`install_from`] tries them best first and demotes the ones that fail: a mirror
/// that served refused bytes goes after every mirror that has not, then fewer failures first,
/// then the caller's order. Keep one `Mirrors` across installs so the demotion lasts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mirrors {
    list: Vec<Mirror>,
}

impl Mirrors {
    /// Mirrors from base URLs, best first.
    pub fn new<S: Into<String>>(bases: impl IntoIterator<Item = S>) -> Self {
        Self {
            list: bases
                .into_iter()
                .map(|b| Mirror {
                    base: b.into(),
                    failures: 0,
                    refused: 0,
                })
                .collect(),
        }
    }

    /// The mirrors, in the caller's order, with their counts.
    #[must_use]
    pub fn mirrors(&self) -> &[Mirror] {
        &self.list
    }

    /// Counts an attempt on mirror `i` that failed on the network or the protocol.
    #[cfg(feature = "store")]
    pub(crate) fn failed(&mut self, i: usize) {
        if let Some(m) = self.list.get_mut(i) {
            m.failures += 1;
        }
    }

    /// Counts an attempt on mirror `i` whose bytes were refused.
    #[cfg(feature = "store")]
    pub(crate) fn refused(&mut self, i: usize) {
        if let Some(m) = self.list.get_mut(i) {
            m.refused += 1;
        }
    }

    /// The base URL of mirror `i`.
    #[cfg(feature = "store")]
    pub(crate) fn base(&self, i: usize) -> Option<&str> {
        self.list.get(i).map(|m| m.base.as_str())
    }

    /// Indexes into [`Mirrors::mirrors`], best first.
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        let mut o: Vec<(u32, u32, usize)> = self
            .list
            .iter()
            .enumerate()
            .map(|(i, m)| (m.refused, m.failures, i))
            .collect();
        o.sort_unstable();
        o.into_iter().map(|(_, _, i)| i).collect()
    }
}

/// [`install`] from the first mirror in [`Mirrors::order`] that completes: `path` is the
/// asset's path under each base. When an attempt fails, the mirror is demoted and the next one
/// resumes from what is staged, so a switch costs the seek table, the head and the block hashes
/// again, never a checked frame. Each attempt fetches its seek table and frames from one
/// mirror, so a lying table cannot make an honest mirror's frames look bad. Every mirror is
/// tried once; retrying later (with backoff) is the caller's, and resumes as well.
///
/// # Errors
/// [`FetchError::Io`] at once (the staging file is not a mirror's fault), or the last mirror's
/// error when every one failed; [`FetchError::Response`] when there is no mirror.
pub fn install_from<T: Transport, F: Read + Write + Seek>(
    transport: &mut T,
    mirrors: &mut Mirrors,
    path: &str,
    entry: &Entry,
    asset_len: u64,
    set: impl Into<Selection>,
    staging: &mut F,
) -> Result<Installed, FetchError> {
    let asset = Asset {
        path,
        entry,
        len: asset_len,
    };
    from_mirrors(transport, mirrors, asset, set.into(), staging, None)
}

/// [`install_from`] for an update (D-174): `previous` is the installed release being replaced
/// (or any earlier one), and every frame of the new pack it holds unchanged is checked and
/// reused instead of fetched; [`Installed::reused`] counts them. Resume works as for an install.
///
/// # Errors
/// As [`install_from`]; reading `previous` fails as [`FetchError::Io`].
pub fn update_from<T: Transport, F: Read + Write + Seek>(
    transport: &mut T,
    mirrors: &mut Mirrors,
    asset: Asset<'_>,
    set: impl Into<Selection>,
    staging: &mut F,
    previous: &mut dyn Previous,
) -> Result<Installed, FetchError> {
    from_mirrors(
        transport,
        mirrors,
        asset,
        set.into(),
        staging,
        Some(previous),
    )
}

fn from_mirrors<T: Transport, F: Read + Write + Seek>(
    transport: &mut T,
    mirrors: &mut Mirrors,
    asset: Asset<'_>,
    set: Selection,
    staging: &mut F,
    mut previous: Option<&mut (dyn Previous + '_)>,
) -> Result<Installed, FetchError> {
    let mut last = FetchError::Response("no mirror");
    for i in mirrors.order() {
        let Some(m) = mirrors.list.get_mut(i) else {
            continue;
        };
        let url = alloc::format!("{}{}", m.base, asset.path);
        let prev = previous.as_deref_mut();
        match run(transport, &url, asset.entry, asset.len, set, staging, prev) {
            Ok(done) => return Ok(done),
            Err(e @ (FetchError::Io(_) | FetchError::Needs(_))) => return Err(e),
            Err(e @ FetchError::Install(_)) => {
                m.refused += 1;
                last = e;
            }
            Err(e) => {
                m.failures += 1;
                last = e;
            }
        }
    }
    Err(last)
}

/// Offers each of `frames` still to do that an earlier attempt could have written (it ends within
/// `old_len`) to [`Install::staged`], read back from `staging`.
fn keep<F: Read + Seek>(
    inst: &mut Install,
    staging: &mut F,
    frames: &[usize],
    old_len: u64,
    buf: &mut Vec<u8>,
) -> Result<(), FetchError> {
    for &i in frames {
        let Some(d) = inst.frame(i).map(|f| f.decoded) else {
            continue;
        };
        if !inst.todo(i) || d.end > old_len {
            continue;
        }
        staging.seek(SeekFrom::Start(d.start))?;
        buf.resize(
            usize::try_from(d.end - d.start).map_err(|_| too_large())?,
            0,
        );
        staging.read_exact(buf)?;
        inst.staged(i, buf)?;
    }
    Ok(())
}

const fn too_large() -> FetchError {
    FetchError::Response("a frame too large for this platform")
}

/// The default transport (feature `http`): `ureq` over rustls with Mozilla's roots (D-279),
/// HTTP/1.1, redirects followed with the `Range` header kept (GitHub's asset host redirects).
#[cfg(feature = "http")]
#[derive(Clone, Debug)]
pub struct Ureq {
    agent: ureq::Agent,
}

#[cfg(feature = "http")]
impl Default for Ureq {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "http")]
impl Ureq {
    /// A transport with a 30 s connect, 30 s response-head and 60 s per-read timeout, and
    /// statuses as values (so a 200 answer to a ranged request is seen and refused).
    #[must_use]
    pub fn new() -> Self {
        use core::time::Duration;
        Self::with_timeouts(
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(60),
        )
    }

    /// [`Ureq::new`] with its timeouts: to connect, to receive a response's head, and between
    /// two reads of its body. The head timeout bounds a host that accepts and never answers;
    /// a job's cancel ends at the next chunk, so these bound how long a cancel can wait on a
    /// silent host.
    #[must_use]
    pub fn with_timeouts(
        connect: core::time::Duration,
        head: core::time::Duration,
        read: core::time::Duration,
    ) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(connect))
            .timeout_recv_response(Some(head))
            .timeout_recv_body(Some(read))
            .user_agent(concat!("teigeodata/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

/// The read size of a streamed response.
#[cfg(feature = "http")]
const STREAM_CHUNK: usize = 64 * 1024;

#[cfg(feature = "http")]
impl Transport for Ureq {
    fn get(&mut self, url: &str, range: Range<u64>, out: &mut Vec<u8>) -> Result<Got, FetchError> {
        use std::io::Read;
        out.clear();
        if range.is_empty() {
            return Err(FetchError::Response("an empty range"));
        }
        let header = alloc::format!("bytes={}-{}", range.start, range.end - 1);
        let mut r = self
            .agent
            .get(url)
            .header("Range", &header)
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(|e| FetchError::Transport(alloc::format!("{e}")))?;
        let status = r.status().as_u16();
        if status != 206 {
            return Err(FetchError::Status(status));
        }
        let text = |name: &str| r.headers().get(name).and_then(|v| v.to_str().ok());
        let total = content_range(text("content-range"), &range)?;
        let etag = text("etag").map(String::from);
        let want = range.end - range.start;
        // At most one byte past the range is read, to see a body that runs over.
        r.body_mut()
            .as_reader()
            .take(want + 1)
            .read_to_end(out)
            .map_err(|e| FetchError::Transport(alloc::format!("{e}")))?;
        if out.len() as u64 != want {
            return Err(FetchError::Response("the body is not the range's length"));
        }
        Ok(Got { etag, total })
    }

    fn get_streamed(
        &mut self,
        url: &str,
        range: Range<u64>,
        buf: &mut Vec<u8>,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), FetchError>,
    ) -> Result<Got, FetchError> {
        use std::io::Read;
        if range.is_empty() {
            return Err(FetchError::Response("an empty range"));
        }
        let header = alloc::format!("bytes={}-{}", range.start, range.end - 1);
        let mut r = self
            .agent
            .get(url)
            .header("Range", &header)
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(|e| FetchError::Transport(alloc::format!("{e}")))?;
        let status = r.status().as_u16();
        if status != 206 {
            return Err(FetchError::Status(status));
        }
        let text = |name: &str| r.headers().get(name).and_then(|v| v.to_str().ok());
        let total = content_range(text("content-range"), &range)?;
        let etag = text("etag").map(String::from);
        let want = range.end - range.start;
        // At most one byte past the range is read, to see a body that runs over.
        let mut reader = r.body_mut().as_reader().take(want + 1);
        buf.resize(STREAM_CHUNK, 0);
        let mut seen: u64 = 0;
        loop {
            let n = reader
                .read(buf)
                .map_err(|e| FetchError::Transport(alloc::format!("{e}")))?;
            if n == 0 {
                break;
            }
            seen += n as u64;
            if seen > want {
                return Err(FetchError::Response("the body is not the range's length"));
            }
            sink(buf.get(..n).ok_or_else(too_large)?)?;
        }
        if seen != want {
            return Err(FetchError::Response("the body is not the range's length"));
        }
        Ok(Got { etag, total })
    }

    fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError> {
        use std::io::Read;
        out.clear();
        // No `Accept-Encoding` override: ureq asks for gzip and decodes it (feature `gzip`).
        let mut r = self
            .agent
            .get(url)
            .call()
            .map_err(|e| FetchError::Transport(alloc::format!("{e}")))?;
        let status = r.status().as_u16();
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        r.body_mut()
            .as_reader()
            .take(max.saturating_add(1))
            .read_to_end(out)
            .map_err(|e| FetchError::Transport(alloc::format!("{e}")))?;
        if out.len() as u64 > max {
            out.clear();
            return Err(FetchError::Response("longer than allowed"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FetchError, content_range};
    #[cfg(feature = "http")]
    use alloc::{string::String, vec, vec::Vec};

    #[test]
    fn content_range_names_exactly_the_request() {
        assert_eq!(
            content_range(Some("bytes 0-1023/5000"), &(0..1024)).ok(),
            Some(5000)
        );
        assert_eq!(
            content_range(Some(" bytes 10-10/11 "), &(10..11)).ok(),
            Some(11)
        );
        for (v, r) in [
            (None, 0..1),
            (Some("bytes 0-1023/5000"), 0..1023),
            (Some("bytes 1-1024/5000"), 0..1024),
            (Some("bytes 0-1023/1000"), 0..1024),
            (Some("bytes 0-1023/*"), 0..1024),
            (Some("items 0-1023/5000"), 0..1024),
            (Some("bytes 0-1023"), 0..1024),
            (Some("bytes -1/5"), 0..1),
            (Some("bytes 0-0/5"), 0..0),
            (Some("bytes 0-18446744073709551615/5"), 0..1),
        ] {
            assert!(
                matches!(content_range(v, &r), Err(FetchError::Response(_))),
                "{v:?} for {r:?}"
            );
        }
    }

    /// A server on localhost that answers each connection with the next canned response.
    #[cfg(feature = "http")]
    fn serve(answers: Vec<Vec<u8>>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for answer in answers {
                let (mut c, _) = listener.accept().unwrap();
                let mut req = Vec::new();
                let mut b = [0_u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = c.read(&mut b).unwrap();
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&b[..n]);
                }
                c.write_all(&answer).unwrap();
            }
        });
        alloc::format!("http://{addr}/f")
    }

    #[cfg(feature = "http")]
    fn answer(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut a = alloc::format!(
            "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n{headers}\r\n",
            body.len()
        )
        .into_bytes();
        a.extend_from_slice(body);
        a
    }

    /// `body` as gzip with one stored deflate block (RFC 1951 3.2.4, RFC 1952).
    #[cfg(feature = "http")]
    fn gzip(body: &[u8]) -> Vec<u8> {
        let mut crc = !0_u32;
        for b in body {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
            }
        }
        let len = u16::try_from(body.len()).unwrap();
        let mut g = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255, 1];
        g.extend_from_slice(&len.to_le_bytes());
        g.extend_from_slice(&(!len).to_le_bytes());
        g.extend_from_slice(body);
        g.extend_from_slice(&(!crc).to_le_bytes());
        g.extend_from_slice(&u32::from(len).to_le_bytes());
        g
    }

    /// A host that accepts and never answers fails at the head timeout, not never.
    #[cfg(feature = "http")]
    #[test]
    fn a_silent_host_times_out() {
        use super::{Transport, Ureq};
        use core::time::Duration;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = alloc::format!("http://{}/x", listener.local_addr().unwrap());
        let held = std::thread::spawn(move || {
            // Hold the connection open, unanswered, until the client gives up.
            let (mut c, _) = listener.accept().unwrap();
            let mut sink = [0_u8; 1024];
            while std::io::Read::read(&mut c, &mut sink).is_ok_and(|n| n > 0) {}
        });
        let mut t = Ureq::with_timeouts(
            Duration::from_secs(5),
            Duration::from_millis(300),
            Duration::from_secs(5),
        );
        let at = std::time::Instant::now();
        let mut out = Vec::new();
        assert!(matches!(
            t.get(&url, 0..10, &mut out),
            Err(FetchError::Transport(_))
        ));
        assert!(at.elapsed() < Duration::from_secs(3), "{:?}", at.elapsed());
        held.join().unwrap();
    }

    #[cfg(feature = "http")]
    #[test]
    fn a_fetch_is_whole_capped_after_decoding_and_200_only() {
        use super::{Transport, Ureq};
        let body = b"{\"signed\":{}}".repeat(20);
        let max = body.len() as u64;
        let url = serve(vec![
            answer("200 OK", "", &body),
            answer("200 OK", "content-encoding: gzip\r\n", &gzip(&body)),
            answer("200 OK", "", &[&body[..], b"x"].concat()),
            // Small on the wire, over the cap once decoded.
            answer(
                "200 OK",
                "content-encoding: gzip\r\n",
                &gzip(&[&body[..], b"x"].concat()),
            ),
            answer("404 Not Found", "", b""),
            answer("206 Partial Content", "", &body),
        ]);
        let mut t = Ureq::new();
        let mut out = Vec::new();
        t.fetch(&url, max, &mut out).unwrap();
        assert_eq!(out, body);
        t.fetch(&url, max, &mut out).unwrap();
        assert_eq!(out, body, "gzip is decoded");
        for _ in 0..2 {
            assert!(
                matches!(t.fetch(&url, max, &mut out), Err(FetchError::Response(_))),
                "over the cap"
            );
            assert_eq!(out.len(), 0, "nothing is kept");
        }
        assert!(matches!(
            t.fetch(&url, max, &mut out),
            Err(FetchError::Status(404))
        ));
        assert!(matches!(
            t.fetch(&url, max, &mut out),
            Err(FetchError::Status(206))
        ));
    }
}
