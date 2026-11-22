//! The catalog refresh as one core free of I/O (D-178, D-369): the TUF client workflow (spec
//! §5) written once, awaiting its file and network effects through [`MetaIo`].
//!
//! A driver supplies the effects. The native store's ([`crate::refresh`]) is synchronous and
//! runs the core with [`block_on`], since its futures are ready at once; a browser's awaits
//! `fetch` and the Origin Private File System. The walk, its checks and its commit order are
//! the same for both: no protocol step is written twice.
//!
//! [`refresh`] walks the roots, then takes the timestamp, the snapshot, the top-level targets
//! and the delegated roles of the countries asked for and of every country already held,
//! each from the stored copy when it is still the one the newer metadata names, else from the
//! first mirror that serves bytes the chain accepts. When nothing changed it fetches one small
//! file, `timestamp.json`. [`load`] reads what is stored, with no network, for an app offline.
//!
//! Layout under the store's root: `meta/root.json` (the newest trusted root),
//! `meta/timestamp.json`, and the rest by version: `meta/V.snapshot.json`,
//! `meta/V.targets.json`, `meta/roles/V.<name>.json`. The stored timestamp is the commit
//! point: it is written last, once every file it leads to is stored, and only then are the
//! files of the previous chain deleted. A refresh that fails or is cut off anywhere leaves the
//! last complete catalog for [`load`], which an app offline reads (the offline-first rule).
//! [`MetaIo::put`] writes a file atomically and durably. When a root rotation changes the
//! timestamp or snapshot keys, the stored timestamp is deleted before the new root is stored
//! (spec 5.3.11, recovery from a fast-forward): the new root is never stored beside metadata
//! the rotation meant to drop.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};
use std::io;

use crate::fetch::{FetchError, Mirrors};
use crate::trust::{self, MAX_ROOT_ROTATIONS, RoleName, Trusted, role_name_ok};

const META: &str = "meta";
const ROLES: &str = "meta/roles";
const ROOT: &str = "meta/root.json";
const TIMESTAMP: &str = "meta/timestamp.json";

/// The effects of a refresh: the store's metadata files and the metadata hosts. Paths are
/// relative to the store's root, with `/` between parts.
#[allow(
    async_fn_in_trait,
    reason = "drivers are single-threaded (a browser worker) or ready at once (block_on); no Send bound is wanted"
)]
pub trait MetaIo {
    /// A whole file, or `None` if it does not exist.
    ///
    /// # Errors
    /// The storage's.
    async fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>>;
    /// Writes `data` at `path` atomically (a reader sees the old file or the new one) and
    /// durably.
    ///
    /// # Errors
    /// The storage's.
    async fn put(&mut self, path: &str, data: &[u8]) -> io::Result<()>;
    /// Removes a file; a missing one is not an error.
    ///
    /// # Errors
    /// The storage's.
    async fn remove(&mut self, path: &str) -> io::Result<()>;
    /// The names of the files in a directory; none for a directory that does not exist.
    ///
    /// # Errors
    /// The storage's.
    async fn list(&mut self, dir: &str) -> io::Result<Vec<String>>;
    /// Creates a directory if missing, durably.
    ///
    /// # Errors
    /// The storage's.
    async fn create_dir(&mut self, path: &str) -> io::Result<()>;
    /// Makes a directory's entries durable (`""` is the root); a no-op where the storage is
    /// durable by itself.
    ///
    /// # Errors
    /// The storage's.
    async fn sync_dir(&mut self, path: &str) -> io::Result<()>;
    /// A whole metadata file from `url`, at most `max` bytes, into `out` (cleared first), as
    /// [`crate::fetch::Transport::fetch`].
    ///
    /// # Errors
    /// As [`crate::fetch::Transport::fetch`]: 404 and 403 as [`FetchError::Status`].
    async fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError>;
}

/// Runs a future whose every await is ready at once (a synchronous driver's), on this thread.
///
/// A future that waits on something is polled again until it is done: no driver in this crate
/// does, and a waiting future here means a driver that should be awaited instead.
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

/// Why a refresh stopped. The stored metadata is always left valid: what was verified before
/// the error is kept, nothing unverified is.
#[derive(Debug)]
#[non_exhaustive]
pub enum RefreshError {
    /// The store's files.
    Io(io::Error),
    /// No mirror served the file.
    Network {
        /// The file asked for.
        file: String,
        /// The last mirror's error.
        error: FetchError,
    },
    /// Every mirror's bytes for the file were refused by the trust chain.
    Refused {
        /// The file asked for.
        file: String,
        /// Why the last mirror's bytes were refused.
        error: trust::Error,
    },
}

impl fmt::Display for RefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the store: {e}"),
            Self::Network { file, error } => write!(f, "{file}: {error}"),
            Self::Refused { file, error } => write!(f, "{file}: refused: {error}"),
        }
    }
}

impl core::error::Error for RefreshError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Network { error, .. } => Some(error),
            Self::Refused { error, .. } => Some(error),
        }
    }
}

impl From<io::Error> for RefreshError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A finished refresh.
#[derive(Debug)]
pub struct Refreshed {
    /// The verified metadata, with every role loaded.
    pub trusted: Trusted,
    /// The files fetched, in order (none stored again unchanged).
    pub fetched: Vec<String>,
    /// Their bytes, decoded.
    pub bytes: u64,
}

/// Refreshes the catalog: the roots after the newest of the stored and the `shipped` root,
/// then the timestamp, snapshot, top-level targets and the roles named in `want` and every
/// role already stored, from `mirrors` (metadata hosts, each base ending in `/`). `now` is Unix
/// seconds.
///
/// # Errors
/// [`RefreshError`]: the store, the network, or a refusal; the last complete catalog then
/// stays for [`load`]. A role in `want` the catalog does not delegate is refused with
/// [`trust::Error::NoRole`].
pub async fn refresh<I: MetaIo>(
    io: &mut I,
    mirrors: &mut Mirrors,
    shipped: &[u8],
    now: i64,
    want: &[&str],
) -> Result<Refreshed, RefreshError> {
    dirs(io).await?;
    let mut t = start(io, shipped, now).await?;
    let mut got = Fetcher {
        io,
        mirrors,
        fetched: Vec::new(),
        bytes: 0,
    };

    // 1. Roots: each next version until a mirror says there is none.
    for _ in 0..MAX_ROOT_ROTATIONS {
        let name = t.next_root_name();
        let Some(data) = got
            .fetch(&name, trust::MAX_ROOT_LEN, true, |d| {
                t.update_root(d).map(|()| true)
            })
            .await?
        else {
            break;
        };
        if t.keys_rotated() {
            got.io.remove(TIMESTAMP).await?;
            got.io.sync_dir(META).await?;
        }
        got.io.put(ROOT, &data).await?;
    }

    // 2. The committed timestamp (the floor against rollback), then the host's.
    if let Some(d) = got.io.read(TIMESTAMP).await? {
        let _ = t.update_timestamp(&d);
    }
    let committed = snapshot_version(&t);
    let max = t.max_len(RoleName::Timestamp);
    let new_timestamp = got
        .fetch("timestamp.json", max, false, |d| {
            match t.update_timestamp(d) {
                Ok(()) => Ok(true),
                Err(trust::Error::Unchanged) => Ok(false),
                Err(e) => Err(e),
            }
        })
        .await?;

    // 3. The snapshot: the committed one first, as the floor for the targets' versions; the
    //    one the timestamp names from the store (a cut-off refresh may have left it) or a host.
    if let Some(v) = committed
        && let Some(d) = got.io.read(&snapshot_path(v)).await?
    {
        let _ = t.update_snapshot(&d, true);
    }
    let named = |t: &Trusted| t.snapshot().map(|s| s.common.version) == snapshot_version(t);
    if !named(&t) {
        let v = snapshot_version(&t).ok_or(RefreshError::Refused {
            file: "timestamp.json".into(),
            error: trust::Error::Order("no timestamp"),
        })?;
        let remote = t.snapshot_name().map_err(refused("snapshot.json"))?;
        let max = t.max_len(RoleName::Snapshot);
        take(&mut got, &snapshot_path(v), &remote, max, |d| {
            t.update_snapshot(d, false)
        })
        .await?;
    }

    // 4. The top-level targets.
    let v = targets_version(&t).ok_or_else(|| refused("targets.json")(trust::Error::NoRole))?;
    let remote = t.targets_name().map_err(refused("targets.json"))?;
    let max = t.max_len(RoleName::Targets);
    take(&mut got, &targets_path(v), &remote, max, |d| {
        t.update_targets(d)
    })
    .await?;

    // 5. The roles asked for and the roles held.
    let held = roles(&mut got, &mut t, want).await?;

    // 6. Commit, then drop what the new chain no longer names.
    if let Some(ts) = new_timestamp {
        got.io.put(TIMESTAMP, &ts).await?;
    }
    collect(got.io, &t, &held).await?;
    Ok(Refreshed {
        trusted: t,
        fetched: got.fetched,
        bytes: got.bytes,
    })
}

/// The roles asked for and the roles held, each from its stored file when current; the names
/// of the roles held after it.
async fn roles<I: MetaIo>(
    got: &mut Fetcher<'_, I>,
    t: &mut Trusted,
    want: &[&str],
) -> Result<Vec<String>, RefreshError> {
    for name in want {
        if role_version(t, name).is_none() || t.delegated_name(name).is_err() {
            return Err(RefreshError::Refused {
                file: alloc::format!("{name}.json"),
                error: trust::Error::NoRole,
            });
        }
    }
    let mut names: Vec<String> = stored_roles(got.io)
        .await?
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| t.delegated_name(n).is_ok())
        .collect();
    for name in want {
        names.push((*name).to_string());
    }
    names.sort();
    names.dedup();
    for name in &names {
        let v = role_version(t, name).ok_or_else(|| refused(name)(trust::Error::NoRole))?;
        let remote = t.delegated_name(name).map_err(refused(name))?;
        let max = t.max_delegated_len(name);
        take(got, &role_path(v, name), &remote, max, |d| {
            t.update_delegated(name, d)
        })
        .await?;
    }
    Ok(names)
}

/// The file at `path` if `accept` takes it, else `remote` from a host, stored at `path`.
async fn take<I: MetaIo>(
    got: &mut Fetcher<'_, I>,
    path: &str,
    remote: &str,
    max: u64,
    mut accept: impl FnMut(&[u8]) -> Result<(), trust::Error>,
) -> Result<(), RefreshError> {
    if got.io.read(path).await?.is_some_and(|d| accept(&d).is_ok()) {
        return Ok(());
    }
    if let Some(data) = got
        .fetch(remote, max, false, |d| accept(d).map(|()| true))
        .await?
    {
        got.io.put(path, &data).await?;
    }
    Ok(())
}

/// Removes every versioned file the trusted chain does not name, and the roles not `held`.
async fn collect<I: MetaIo>(io: &mut I, t: &Trusted, held: &[String]) -> io::Result<()> {
    let keep_sn = snapshot_version(t).map(snapshot_path);
    let keep_tg = targets_version(t).map(targets_path);
    let mut changed = false;
    for f in io.list(META).await? {
        let path = alloc::format!("{META}/{f}");
        let ours = f.ends_with(".snapshot.json") || f.ends_with(".targets.json");
        if ours && Some(&path) != keep_sn.as_ref() && Some(&path) != keep_tg.as_ref() {
            io.remove(&path).await?;
            changed = true;
        }
    }
    if changed {
        io.sync_dir(META).await?;
    }
    let mut changed = false;
    for (v, name) in stored_roles(io).await? {
        let keep = held.contains(&name) && role_version(t, &name) == Some(v);
        if !keep {
            io.remove(&role_path(v, &name)).await?;
            changed = true;
        }
    }
    if changed {
        io.sync_dir(ROLES).await?;
    }
    Ok(())
}

/// The stored metadata, with no network: the last complete catalog, which an app offline
/// reads. Expired metadata loads as far as the chain keeps it (an expired timestamp or
/// snapshot blocks the files after it: nothing may be installed from them).
///
/// # Errors
/// [`RefreshError::Io`]; [`RefreshError::Refused`] when neither the stored nor the shipped
/// root verifies.
pub async fn load<I: MetaIo>(
    io: &mut I,
    shipped: &[u8],
    now: i64,
) -> Result<Trusted, RefreshError> {
    let mut t = start(io, shipped, now).await?;
    if let Some(d) = io.read(TIMESTAMP).await? {
        let _ = t.update_timestamp(&d);
    }
    if let Some(v) = snapshot_version(&t)
        && let Some(d) = io.read(&snapshot_path(v)).await?
    {
        let _ = t.update_snapshot(&d, false);
    }
    if let Some(v) = targets_version(&t)
        && let Some(d) = io.read(&targets_path(v)).await?
    {
        let _ = t.update_targets(&d);
    }
    for (v, name) in stored_roles(io).await? {
        if role_version(&t, &name) == Some(v)
            && let Some(d) = io.read(&role_path(v, &name)).await?
        {
            let _ = t.update_delegated(&name, &d);
        }
    }
    Ok(t)
}

fn snapshot_version(t: &Trusted) -> Option<u64> {
    t.timestamp().map(|x| x.snapshot.version)
}

fn targets_version(t: &Trusted) -> Option<u64> {
    t.snapshot()?.meta.get("targets.json").map(|m| m.version)
}

fn role_version(t: &Trusted, name: &str) -> Option<u64> {
    t.snapshot()?
        .meta
        .get(&alloc::format!("{name}.json"))
        .map(|m| m.version)
}

fn snapshot_path(v: u64) -> String {
    alloc::format!("{META}/{v}.snapshot.json")
}

fn targets_path(v: u64) -> String {
    alloc::format!("{META}/{v}.targets.json")
}

fn role_path(v: u64, name: &str) -> String {
    alloc::format!("{ROLES}/{v}.{name}.json")
}

/// The trusted root to start from: the newer of the stored and the shipped one (an app update
/// may ship a newer root); a stored root that does not verify is ignored.
async fn start<I: MetaIo>(io: &mut I, shipped: &[u8], now: i64) -> Result<Trusted, RefreshError> {
    let from_app = Trusted::new(shipped, now).map_err(refused("the shipped root"))?;
    let stored = io
        .read(ROOT)
        .await?
        .and_then(|d| Trusted::new(&d, now).ok())
        .filter(|s| s.root().common.version > from_app.root().common.version);
    Ok(stored.unwrap_or(from_app))
}

fn refused(file: &str) -> impl Fn(trust::Error) -> RefreshError + '_ {
    move |error| RefreshError::Refused {
        file: file.to_string(),
        error,
    }
}

/// The stored roles as (version, name); a file not of that form is not ours and is left.
async fn stored_roles<I: MetaIo>(io: &mut I) -> io::Result<Vec<(u64, String)>> {
    Ok(io
        .list(ROLES)
        .await?
        .into_iter()
        .filter_map(|f| {
            let (v, rest) = f.split_once('.')?;
            let name = rest.strip_suffix(".json")?;
            let v: u64 = v.parse().ok()?;
            role_name_ok(name).then(|| (v, name.to_string()))
        })
        .collect())
}

/// `meta/` and `meta/roles/`, durable.
async fn dirs<I: MetaIo>(io: &mut I) -> io::Result<()> {
    io.create_dir(META).await?;
    io.create_dir(ROLES).await?;
    io.sync_dir("").await?;
    io.sync_dir(META).await
}

/// Fetches metadata files from the mirrors, best first, demoting the ones that fail.
struct Fetcher<'a, I: MetaIo> {
    io: &'a mut I,
    mirrors: &'a mut Mirrors,
    fetched: Vec<String>,
    bytes: u64,
}

impl<I: MetaIo> Fetcher<'_, I> {
    /// `file` from the first mirror whose bytes `accept` takes: `Some` with them when `accept`
    /// says to store them, `None` when it took them but they are nothing new, or when `missing`
    /// is allowed and a mirror answers 404 or 403 (the end of a root walk). A mirror whose
    /// bytes are refused, or that fails, is demoted and the next one tried.
    async fn fetch(
        &mut self,
        file: &str,
        max: u64,
        missing: bool,
        mut accept: impl FnMut(&[u8]) -> Result<bool, trust::Error>,
    ) -> Result<Option<Vec<u8>>, RefreshError> {
        let mut net: Option<FetchError> = None;
        let mut bad: Option<trust::Error> = None;
        let mut buf = Vec::new();
        for i in self.mirrors.order() {
            let Some(base) = self.mirrors.base(i) else {
                continue;
            };
            let url = alloc::format!("{base}{file}");
            match self.io.fetch(&url, max, &mut buf).await {
                Ok(()) => {
                    self.fetched.push(file.to_string());
                    self.bytes += buf.len() as u64;
                    match accept(&buf) {
                        Ok(true) => return Ok(Some(buf)),
                        Ok(false) => return Ok(None),
                        Err(e) => {
                            self.mirrors.refused(i);
                            bad = Some(e);
                        }
                    }
                }
                Err(FetchError::Status(404 | 403)) if missing => return Ok(None),
                Err(e) => {
                    self.mirrors.failed(i);
                    net = Some(e);
                }
            }
        }
        Err(match (bad, net) {
            (Some(error), _) => RefreshError::Refused {
                file: file.to_string(),
                error,
            },
            (None, error) => RefreshError::Network {
                file: file.to_string(),
                error: error.unwrap_or(FetchError::Response("no mirror")),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::block_on;
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll};

    /// A future that is not ready the first `n` times it is polled.
    struct Later(u32);

    impl Future for Later {
        type Output = u32;
        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<u32> {
            if self.0 == 0 {
                Poll::Ready(7)
            } else {
                self.0 -= 1;
                Poll::Pending
            }
        }
    }

    #[test]
    fn block_on_runs_a_ready_future_and_one_that_waits() {
        assert_eq!(block_on(async { 3 }), 3);
        assert_eq!(block_on(Later(5)), 7);
    }
}
