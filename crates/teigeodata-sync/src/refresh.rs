//! The catalog refresh (D-178): the TUF client workflow (spec §5) over a [`Transport`], with
//! the verified metadata kept in the store under `meta/`, so every app and the CLI share one
//! tested path ([the CLI note](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/cli.md#5-what-we-build-ourselves-and-why)).
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
//! Each file is written to `meta/tmp`, synced, renamed over its name and its directory synced.
//! When a root rotation changes the timestamp or snapshot keys, the stored timestamp is deleted
//! before the new root is stored (spec 5.3.11, recovery from a fast-forward): //! the new root is never stored beside metadata the rotation meant to drop.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use std::io::{self, Write as _};

use crate::fetch::{FetchError, Mirrors, Transport};
use crate::store::{Fs, StoreFile};
use crate::trust::{self, MAX_ROOT_ROTATIONS, RoleName, Trusted, role_name_ok};

const META: &str = "meta";
const ROLES: &str = "meta/roles";
const ROOT: &str = "meta/root.json";
const TIMESTAMP: &str = "meta/timestamp.json";
const TMP: &str = "meta/tmp";

/// Why a refresh stopped. The stored metadata is always left valid: what was verified before
/// the error is kept, nothing unverified is.
#[derive(Debug)]
#[non_exhaustive]
pub enum RefreshError {
    /// The store's file system.
    Io(io::Error),
    /// No mirror could serve `file`: the network, or a status other than 200.
    Network {
        /// The metadata file.
        file: String,
        /// The last mirror's error.
        error: FetchError,
    },
    /// The trust chain refused `file`, from every mirror that served it.
    Refused {
        /// The metadata file, or the role asked for.
        file: String,
        /// Why.
        error: trust::Error,
    },
}

impl fmt::Display for RefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the store: {e}"),
            Self::Network { file, error } => write!(f, "{file}: {error}"),
            Self::Refused { file, error } => write!(f, "{file}: {error}"),
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
pub fn refresh<D: Fs, T: Transport>(
    fs: &mut D,
    transport: &mut T,
    mirrors: &mut Mirrors,
    shipped: &[u8],
    now: i64,
    want: &[&str],
) -> Result<Refreshed, RefreshError> {
    dirs(fs)?;
    let mut got = Fetcher {
        transport,
        mirrors,
        fetched: Vec::new(),
        bytes: 0,
    };
    let mut t = start(fs, shipped, now)?;

    // 1. Roots: each next version until a mirror says there is none.
    for _ in 0..MAX_ROOT_ROTATIONS {
        let name = t.next_root_name();
        let Some(data) = got.fetch(&name, trust::MAX_ROOT_LEN, true, |d| {
            t.update_root(d).map(|()| true)
        })?
        else {
            break;
        };
        if t.keys_rotated() {
            fs.remove(TIMESTAMP)?;
            fs.sync_dir(META)?;
        }
        put(fs, ROOT, &data)?;
    }

    // 2. The committed timestamp (the floor against rollback), then the host's.
    if let Some(d) = fs.read(TIMESTAMP)? {
        let _ = t.update_timestamp(&d);
    }
    let committed = snapshot_version(&t);
    let max = t.max_len(RoleName::Timestamp);
    let new_timestamp = got.fetch("timestamp.json", max, false, |d| {
        match t.update_timestamp(d) {
            Ok(()) => Ok(true),
            Err(trust::Error::Unchanged) => Ok(false),
            Err(e) => Err(e),
        }
    })?;

    // 3. The snapshot: the committed one first, as the floor for the targets' versions; the
    //    one the timestamp names from the store (a cut-off refresh may have left it) or a host.
    if let Some(v) = committed
        && let Some(d) = fs.read(&snapshot_path(v))?
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
        take(fs, &mut got, &snapshot_path(v), &remote, max, |d| {
            t.update_snapshot(d, false)
        })?;
    }

    // 4. The top-level targets.
    let v = targets_version(&t).ok_or_else(|| refused("targets.json")(trust::Error::NoRole))?;
    let remote = t.targets_name().map_err(refused("targets.json"))?;
    let max = t.max_len(RoleName::Targets);
    take(fs, &mut got, &targets_path(v), &remote, max, |d| {
        t.update_targets(d)
    })?;

    // 5. The roles asked for and the roles held.
    let held = roles(fs, &mut got, &mut t, want)?;

    // 6. Commit, then drop what the new chain no longer names.
    if let Some(ts) = new_timestamp {
        put(fs, TIMESTAMP, &ts)?;
    }
    collect(fs, &t, &held)?;
    Ok(Refreshed {
        trusted: t,
        fetched: got.fetched,
        bytes: got.bytes,
    })
}

/// The roles asked for and the roles held, each from its stored file when current; the names
/// of the roles held after it.
fn roles<D: Fs, T: Transport>(
    fs: &mut D,
    got: &mut Fetcher<'_, T>,
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
    let mut names: Vec<String> = stored_roles(fs)?
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
        take(fs, got, &role_path(v, name), &remote, max, |d| {
            t.update_delegated(name, d)
        })?;
    }
    Ok(names)
}

/// The file at `path` if `accept` takes it, else `remote` from a host, stored at `path`.
fn take<D: Fs, T: Transport>(
    fs: &mut D,
    got: &mut Fetcher<'_, T>,
    path: &str,
    remote: &str,
    max: u64,
    mut accept: impl FnMut(&[u8]) -> Result<(), trust::Error>,
) -> Result<(), RefreshError> {
    if fs.read(path)?.is_some_and(|d| accept(&d).is_ok()) {
        return Ok(());
    }
    if let Some(data) = got.fetch(remote, max, false, |d| accept(d).map(|()| true))? {
        put(fs, path, &data)?;
    }
    Ok(())
}

/// Removes every versioned file the trusted chain does not name, and the roles not `held`.
fn collect<D: Fs>(fs: &mut D, t: &Trusted, held: &[String]) -> io::Result<()> {
    let keep_sn = snapshot_version(t).map(snapshot_path);
    let keep_tg = targets_version(t).map(targets_path);
    let mut changed = false;
    for f in fs.list(META)? {
        let path = alloc::format!("{META}/{f}");
        let ours = f.ends_with(".snapshot.json") || f.ends_with(".targets.json");
        if ours && Some(&path) != keep_sn.as_ref() && Some(&path) != keep_tg.as_ref() {
            fs.remove(&path)?;
            changed = true;
        }
    }
    if changed {
        fs.sync_dir(META)?;
    }
    let mut changed = false;
    for (v, name) in stored_roles(fs)? {
        let keep = held.contains(&name) && role_version(t, &name) == Some(v);
        if !keep {
            fs.remove(&role_path(v, &name))?;
            changed = true;
        }
    }
    if changed {
        fs.sync_dir(ROLES)?;
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
pub fn load<D: Fs>(fs: &mut D, shipped: &[u8], now: i64) -> Result<Trusted, RefreshError> {
    let mut t = start(fs, shipped, now)?;
    if let Some(d) = fs.read(TIMESTAMP)? {
        let _ = t.update_timestamp(&d);
    }
    if let Some(v) = snapshot_version(&t)
        && let Some(d) = fs.read(&snapshot_path(v))?
    {
        let _ = t.update_snapshot(&d, false);
    }
    if let Some(v) = targets_version(&t)
        && let Some(d) = fs.read(&targets_path(v))?
    {
        let _ = t.update_targets(&d);
    }
    for (v, name) in stored_roles(fs)? {
        if role_version(&t, &name) == Some(v)
            && let Some(d) = fs.read(&role_path(v, &name))?
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
fn start<D: Fs>(fs: &mut D, shipped: &[u8], now: i64) -> Result<Trusted, RefreshError> {
    let from_app = Trusted::new(shipped, now).map_err(refused("the shipped root"))?;
    let stored = fs
        .read(ROOT)?
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
fn stored_roles<D: Fs>(fs: &mut D) -> io::Result<Vec<(u64, String)>> {
    Ok(fs
        .list(ROLES)?
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
fn dirs<D: Fs>(fs: &mut D) -> io::Result<()> {
    fs.create_dir(META)?;
    fs.create_dir(ROLES)?;
    fs.sync_dir("")?;
    fs.sync_dir(META)
}

/// `data` at `path`, atomically and durably.
fn put<D: Fs>(fs: &mut D, path: &str, data: &[u8]) -> io::Result<()> {
    fs.remove(TMP)?;
    let mut f = fs.open(TMP)?;
    f.set_len(0)?;
    f.write_all(data)?;
    f.sync()?;
    drop(f);
    fs.rename(TMP, path)?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    fs.sync_dir(dir)
}

/// Fetches metadata files from the mirrors, best first, demoting the ones that fail.
struct Fetcher<'a, T: Transport> {
    transport: &'a mut T,
    mirrors: &'a mut Mirrors,
    fetched: Vec<String>,
    bytes: u64,
}

impl<T: Transport> Fetcher<'_, T> {
    /// `file` from the first mirror whose bytes `accept` takes: `Some` with them when `accept`
    /// says to store them, `None` when it took them but they are nothing new, or when `missing`
    /// is allowed and a mirror answers 404 or 403 (the end of a root walk). A mirror whose
    /// bytes are refused, or that fails, is demoted and the next one tried.
    fn fetch(
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
            match self.transport.fetch(&url, max, &mut buf) {
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
