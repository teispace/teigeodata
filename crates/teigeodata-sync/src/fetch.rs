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
use std::io::{Seek, SeekFrom, Write};

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
/// target's length) over `transport`, into `staging` (truncated and sized to the pack; sparse
/// where nothing is installed), then flushes it to disk ([`std::fs::File::sync_all`] is the
/// caller's for a file it renames, as staging is).
///
/// # Errors
/// The first [`FetchError`]: a request, a response that is not exactly its range, an asset whose
/// length or `ETag` changes, the install's checks, or the file.
pub fn install<T: Transport, F: Write + Seek>(
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
    let mut write = |at: u64, b: &[u8]| -> Result<(), FetchError> {
        staging.seek(SeekFrom::Start(at))?;
        staging.write_all(b)?;
        Ok(())
    };
    let [t, h] = inst.first()?;
    let (mut table, mut head) = (Vec::new(), Vec::new());
    get(transport, t, &mut table)?;
    get(transport, h, &mut head)?;
    let fetches = inst.start(&table, &head, &mut write)?.to_vec();
    let mut buf = Vec::new();
    for (i, f) in fetches.iter().enumerate() {
        get(transport, f.compressed.clone(), &mut buf)?;
        inst.feed(i, &buf, &mut write)?;
    }
    staging.flush()?;
    Ok(inst.finish()?)
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
