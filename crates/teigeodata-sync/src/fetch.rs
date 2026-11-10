//! Moving the bytes (D-168): a [`Transport`] makes one ranged request, and [`install`] drives the
//! sans-I/O [`Install`] over one into a staging file.
//!
//! A transport is small on purpose: platform apps bring their own (`URLSession`, `OkHttp`, `fetch`)
//! behind the same trait or drive [`Install`] directly. [`Ureq`] (feature `http`) is the
//! default for desktop and servers. Every response is held to the request: status 206, a
//! `Content-Range` naming exactly the requested bytes and the asset's length, at most those
//! bytes read, and one `ETag` for the whole install, so a host that ignores `Range` or swaps the
//! asset midway is refused, not trusted (the GitHub asset host ignores `If-Range`).

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;
use std::io::{Read, Seek, SeekFrom, Write};

use teigeodata_format::component::Components;

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

/// One ranged GET.
pub trait Transport {
    /// Fetches exactly the bytes `range` of the asset at `url` into `out` (cleared first).
    ///
    /// # Errors
    /// A [`FetchError`]: the request failed, or the response was not exactly `range`.
    fn get(&mut self, url: &str, range: Range<u64>, out: &mut Vec<u8>) -> Result<Got, FetchError>;
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
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(s) => write!(f, "the host answered {s}, not 206 Partial Content"),
            Self::Response(p) => write!(f, "the host's response was refused: {p}"),
            Self::Transport(m) => write!(f, "the request failed: {m}"),
            Self::Io(e) => write!(f, "the staging file: {e}"),
            Self::Install(e) => write!(f, "{e}"),
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
    set: Components,
    staging: &mut F,
) -> Result<Installed, FetchError> {
    let mut inst = Install::new(entry, asset_len, set);
    let mut etag: Option<Option<String>> = None;
    let mut get = |transport: &mut T, range: Range<u64>, out: &mut Vec<u8>| {
        let got = transport.get(url, range, out)?;
        if got.total != asset_len {
            return Err(FetchError::Response(
                "the asset's length is not the catalog's",
            ));
        }
        match &etag {
            Some(e) if *e != got.etag => Err(FetchError::Response(
                "the asset changed on the host during the install",
            )),
            Some(_) => Ok(()),
            None => {
                etag = Some(got.etag);
                Ok(())
            }
        }
    };
    let [t, h] = inst.first()?;
    let (mut table, mut head) = (Vec::new(), Vec::new());
    get(transport, t, &mut table)?;
    get(transport, h, &mut head)?;
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
    if old_len > 0 {
        // BLOCK_HASHES first (the plan lists it first), always fetched; then keep what checks.
        while !inst.proven() {
            let Some(f) = inst.remaining().into_iter().next() else {
                break;
            };
            get(transport, f.compressed.clone(), &mut buf)?;
            inst.feed(&f, &buf, |at, b| write(staging, at, b))?;
        }
        let frames: Vec<usize> = plan.iter().flat_map(|f| f.frames.clone()).collect();
        keep(&mut inst, staging, &frames, old_len, &mut buf)?;
    }
    for f in inst.remaining() {
        get(transport, f.compressed.clone(), &mut buf)?;
        inst.feed(&f, &buf, |at, b| write(staging, at, b))?;
    }
    staging.flush()?;
    Ok(inst.finish()?)
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
    set: Components,
    staging: &mut F,
) -> Result<Installed, FetchError> {
    let mut last = FetchError::Response("no mirror");
    for i in mirrors.order() {
        let Some(m) = mirrors.list.get_mut(i) else {
            continue;
        };
        let url = alloc::format!("{}{path}", m.base);
        match install(transport, &url, entry, asset_len, set, staging) {
            Ok(done) => return Ok(done),
            Err(e @ FetchError::Io(_)) => return Err(e),
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

/// The default transport (feature `http`): `ureq` over rustls with the platform's trust store,
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
    /// A transport with a 30 s connect and 60 s per-read timeout, and statuses as values (so a
    /// 200 answer to a ranged request is seen and refused).
    #[must_use]
    pub fn new() -> Self {
        use core::time::Duration;
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .user_agent(concat!("teigeodata/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

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
}

#[cfg(test)]
mod tests {
    use super::{FetchError, content_range};

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
}
