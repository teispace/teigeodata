//! The store's client: the policy over one store directory, shared by the command line and
//! every binding ([store API](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/store-api.md)).
//!
//! ```text
//! <store>/config.json   the metadata and asset mirrors, from `init`
//! <store>/root.json     the trusted root `init` was given: the trust anchor
//! <store>/meta/         the verified catalog (`refresh`, D-179)
//! <store>/journal, packs/, staging/   the installed packs (`store`, D-171)
//! ```
//!
//! The catalog works offline once refreshed: [`Client::offline`], [`Client::state`] and
//! [`Client::status`] never touch the network. Nothing here prints: warnings come back in the
//! results.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::{Future, ready};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use teigeodata_format::Part;
use teigeodata_format::component::Selection;

use crate::catalog::{self, Entry};
use crate::fetch::{Downloads, FetchError, Mirrors, Transport, Ureq};
use crate::meta::block_on;
use crate::policy::{self, Outcome, Packs, target};
pub use crate::policy::{ClientError, Done, PackState, Pending, Report, Result, Want, split};
use crate::refresh::{self, Refreshed};
use crate::store::{Dir, Health, Record, Store};
use crate::trust::Trusted;

/// The store's configuration, written by [`init`].
pub const CONFIG: &str = "config.json";
/// The trust anchor [`init`] was given.
pub const ROOT: &str = "root.json";

/// Seconds since the Unix epoch, for the catalog's expiry checks.
#[must_use]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The verified catalog stored at `root` and the packs its journal lists, read without
/// opening the store: nothing is recovered, created or removed, so a reader beside the process
/// that installs (a server answering `coverage`, D-318) never disturbs it. Offline.
///
/// # Errors
/// [`ClientError::Refused`] if the store is not set up, or neither the stored nor the shipped
/// root verifies; the file system's.
pub fn catalog_at(root: &Path) -> Result<(Trusted, Vec<Record>)> {
    let shipped = std::fs::read(root.join(ROOT)).map_err(|e| {
        ClientError::Refused(format!(
            "{}: {e} (set the store up with init first)",
            root.join(ROOT).display()
        ))
    })?;
    let mut dir = Dir::new(root).map_err(|e| ClientError::of(root.display(), e))?;
    let trusted = refresh::load(&mut dir, &shipped, now())?;
    let packs = Store::read_packs(&mut dir).map_err(|e| ClientError::of(root.display(), e))?;
    Ok((trusted, packs))
}

/// A mirror base ends with `/`, so a file name joins it.
fn base(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

/// The catalog path a person names: `np` is `np.places.tgd`, `np.dem` is `np.dem.tgd`, and a
/// path ending in `.tgd` is itself.
#[must_use]
pub fn path_of(name: &str) -> String {
    match name.strip_suffix(".tgd") {
        Some(_) => name.to_owned(),
        None => match name.split_once('.') {
            Some((a, c)) => catalog::path(a, c),
            None => catalog::path(name, "places"),
        },
    }
}

/// A names part for people: its language (`ne`), `*` for the rest, `base` for the base.
#[must_use]
pub fn language(p: Part) -> String {
    if p.is_base() {
        return "base".into();
    }
    if p == Part::REST {
        return "*".into();
    }
    p.language_code().map_or_else(
        || format!("{:#x}", p.0),
        |c| {
            c.iter()
                .filter(|&&b| b != 0)
                .map(|&b| char::from(b))
                .collect()
        },
    )
}

/// Sets up a store at `root`: its metadata and asset mirrors, and the trusted root that
/// becomes its trust anchor once it verifies by itself.
///
/// # Errors
/// [`ClientError::Refused`] if the root does not verify or the files cannot be written.
pub fn init(root: &Path, repo: &[String], assets: &[String], trusted_root: &[u8]) -> Result<()> {
    Trusted::new(trusted_root, now()).map_err(|e| ClientError::of("the trusted root", e))?;
    std::fs::create_dir_all(root).map_err(|e| ClientError::of(root.display(), e))?;
    let config = json!({ "repo": repo, "assets": assets });
    let config = serde_json::to_vec_pretty(&config).map_err(|e| ClientError::of(CONFIG, e))?;
    for (name, data) in [(ROOT, trusted_root), (CONFIG, config.as_slice())] {
        let (path, tmp) = (root.join(name), root.join(format!("{name}.tmp")));
        std::fs::write(&tmp, data)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| ClientError::of(path.display(), e))?;
    }
    Ok(())
}

/// What a repair found and did.
#[derive(Debug)]
pub struct Repaired {
    /// The pack's health before.
    pub health: Health,
    /// Bytes fetched to repair it; `None` when it was whole.
    pub fetched: Option<u64>,
}

/// The store's state.
#[derive(Debug)]
pub struct Status {
    /// The catalog's timestamp version and expiry (Unix seconds); `None` before a refresh.
    pub catalog: Option<(u64, i64)>,
    /// Each installed pack and its health.
    pub packs: Vec<(Record, Health)>,
}

/// An open store: its packs, its catalog's trust anchor, its mirrors and its transport.
#[derive(Debug)]
pub struct Client<N: Transport = Ureq> {
    root: PathBuf,
    store: Store<Dir>,
    shipped: Vec<u8>,
    repo: Vec<String>,
    assets: Vec<String>,
    net: N,
}

impl Client<Ureq> {
    /// The store at `root`, set up by [`init`], over HTTPS. Also returns the packs found
    /// damaged when the store recovered: each needs [`Client::repair`].
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the store is not set up or cannot be read.
    pub fn open(root: &Path) -> Result<(Self, Vec<String>)> {
        Self::open_with(root, Ureq::new())
    }
}

impl<N: Transport> Client<N> {
    /// [`Client::open`] over the transport `net`.
    ///
    /// # Errors
    /// As [`Client::open`].
    pub fn open_with(root: &Path, net: N) -> Result<(Self, Vec<String>)> {
        let at = |p: &str| root.join(p);
        let config = std::fs::read(at(CONFIG)).map_err(|e| {
            ClientError::Refused(format!(
                "{}: {e} (set the store up with init first)",
                at(CONFIG).display()
            ))
        })?;
        let config: Value = serde_json::from_slice(&config)
            .map_err(|e| ClientError::of(at(CONFIG).display(), e))?;
        let list = |k: &str| -> Vec<String> {
            config
                .get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(base).collect())
                .unwrap_or_default()
        };
        let shipped =
            std::fs::read(at(ROOT)).map_err(|e| ClientError::of(at(ROOT).display(), e))?;
        let dir = Dir::new(root).map_err(|e| ClientError::of(root.display(), e))?;
        let (store, recovered) =
            Store::open(dir).map_err(|e| ClientError::of(root.display(), e))?;
        let client = Self {
            root: root.to_path_buf(),
            store,
            shipped,
            repo: list("repo"),
            assets: list("assets"),
            net,
        };
        Ok((client, recovered.damaged))
    }

    /// The store's directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The installed packs.
    #[must_use]
    pub fn packs(&self) -> &[Record] {
        self.store.packs()
    }

    /// Where an installed pack's file is.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the store's directory cannot be read.
    pub fn pack_path(&self, record: &Record) -> Result<PathBuf> {
        let dir = Dir::new(&self.root).map_err(|e| ClientError::of(self.root.display(), e))?;
        Ok(dir.pack_path(record))
    }

    /// The transport, for a caller that watches it.
    pub fn net(&mut self) -> &mut N {
        &mut self.net
    }

    /// The verified catalog, from the store alone.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the stored metadata does not verify.
    pub fn offline(&mut self) -> Result<Trusted> {
        Ok(refresh::load(self.store.fs(), &self.shipped, now())?)
    }

    /// The catalog refreshed from the mirrors, with the roles of `want` (country codes) and of
    /// every country whose pack is installed.
    ///
    /// # Errors
    /// [`ClientError::Network`] if no mirror answers, [`ClientError::Refused`] if what they
    /// send does not verify.
    pub fn refresh(&mut self, want: &[String]) -> Result<Refreshed> {
        let mut want: Vec<String> = want.to_vec();
        for r in self.store.packs() {
            if let Some(cc) = catalog::country_of(&r.address) {
                want.push(String::from_utf8_lossy(&cc).into_owned());
            }
        }
        want.sort();
        want.dedup();
        let refs: Vec<&str> = want.iter().map(String::as_str).collect();
        let mut mirrors = Mirrors::new(self.repo.clone());
        Ok(refresh::refresh(
            self.store.fs(),
            &mut self.net,
            &mut mirrors,
            &self.shipped,
            now(),
            &refs,
        )?)
    }

    /// What the catalog says of `entry` against the store.
    #[must_use]
    pub fn state(&self, entry: &Entry) -> PackState {
        PackState::of(entry, self.store.packs())
    }

    /// Installs the packs at `paths` (catalog paths) with `want` added to what is installed,
    /// dependencies first, pinning each when `pin`. With `downloads`, a platform's pass
    /// (D-184): the asset ranges are read from that directory, and what is missing comes back
    /// in [`Report::pending`].
    ///
    /// # Errors
    /// [`ClientError::Network`] or [`ClientError::Refused`], from the first pack that fails;
    /// the packs before it stay installed.
    pub fn install(
        &mut self,
        paths: &[String],
        want: &Want,
        pin: bool,
        downloads: Option<&Path>,
    ) -> Result<Report> {
        let mut native = Native {
            client: self,
            downloads,
        };
        block_on(policy::install(&mut native, paths, want, pin))
    }

    /// Updates the packs at `paths` (every installed one when empty) to the catalog's
    /// releases, keeping the components and parts installed. Returns the packs updated.
    ///
    /// # Errors
    /// [`ClientError::Refused`] for a path not installed, else as [`Client::install`].
    pub fn update(&mut self, paths: &[String]) -> Result<Vec<Done>> {
        let mut native = Native {
            client: self,
            downloads: None,
        };
        block_on(policy::update(&mut native, paths))
    }

    /// Removes the pack at `path` and the dependencies nothing else needs. Returns what was
    /// removed.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed or another pack needs it.
    pub fn remove(&mut self, path: &str) -> Result<Vec<Record>> {
        let (a, c) = split(path);
        Ok(self.store.remove(a, c)?)
    }

    /// Pins the pack at `path` (kept when room is made), or unpins it.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed.
    pub fn pin(&mut self, path: &str, on: bool) -> Result<()> {
        let (a, c) = split(path);
        Ok(self.store.pin(a, c, on)?)
    }

    /// Checks the pack at `path` in full and, unless it is whole, fetches what it lacks.
    ///
    /// # Errors
    /// [`ClientError::Network`] or [`ClientError::Refused`].
    pub fn repair(&mut self, path: &str) -> Result<Repaired> {
        let (a, c) = split(path);
        let health = self.store.verify(a, c, true)?;
        if health == Health::Good {
            return Ok(Repaired {
                health,
                fetched: None,
            });
        }
        let release = self
            .store
            .packs()
            .iter()
            .find(|r| r.key() == path)
            .map(|r| r.release.clone())
            .unwrap_or_default();
        let t = match self.offline() {
            Ok(t) if target(&t, path).is_ok_and(|(e, _)| e.release == release) => t,
            _ => self.refresh(&[])?.trusted,
        };
        let (entry, asset_len) = target(&t, path)?;
        let mut mirrors = Mirrors::new(self.assets.clone());
        let done =
            self.store
                .repair(&mut self.net, &mut mirrors, &entry.file, &entry, asset_len)?;
        Ok(Repaired {
            health,
            fetched: Some(done.fetched),
        })
    }

    /// The catalog's freshness and each installed pack's health, from a full check when
    /// `deep`.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if a pack cannot be read.
    pub fn status(&mut self, deep: bool) -> Result<Status> {
        let catalog = self
            .offline()
            .ok()
            .and_then(|t| t.timestamp().map(|x| (x.common.version, x.common.expires)));
        let records: Vec<Record> = self.store.packs().to_vec();
        let mut packs = Vec::with_capacity(records.len());
        for r in records {
            let health = self.store.verify(&r.address, &r.component, deep)?;
            packs.push((r, health));
        }
        Ok(Status { catalog, packs })
    }
}

/// The native store's operations for the policy: the store directory, the mirrors over the
/// client's transport or a platform's downloads (D-184), each future ready at once.
struct Native<'a, N: Transport> {
    client: &'a mut Client<N>,
    downloads: Option<&'a Path>,
}

impl<N: Transport> Packs for Native<'_, N> {
    fn packs(&self) -> &[Record] {
        self.client.store.packs()
    }

    fn refresh(&mut self, want: &[String]) -> impl Future<Output = Result<Trusted>> {
        ready(self.client.refresh(want).map(|r| r.trusted))
    }

    fn offline(&mut self) -> impl Future<Output = Result<Trusted>> {
        ready(self.client.offline())
    }

    fn install(
        &mut self,
        entry: &Entry,
        asset_len: u64,
        set: Selection,
    ) -> impl Future<Output = Result<Outcome>> {
        ready(self.install_now(entry, asset_len, set))
    }

    fn mark(
        &mut self,
        address: &str,
        component: &str,
        asked: bool,
    ) -> impl Future<Output = Result<()>> {
        ready(
            self.client
                .store
                .mark(address, component, asked)
                .map_err(Into::into),
        )
    }

    fn pin(
        &mut self,
        address: &str,
        component: &str,
        on: bool,
    ) -> impl Future<Output = Result<()>> {
        ready(
            self.client
                .store
                .pin(address, component, on)
                .map_err(Into::into),
        )
    }
}

impl<N: Transport> Native<'_, N> {
    fn install_now(&mut self, entry: &Entry, asset_len: u64, set: Selection) -> Result<Outcome> {
        let c = &mut *self.client;
        let Some(dir) = self.downloads else {
            let mut mirrors = Mirrors::new(c.assets.clone());
            c.net.expect(entry.size_of(set.set).wire);
            let done =
                c.store
                    .install(&mut c.net, &mut mirrors, &entry.file, entry, asset_len, set)?;
            return Ok(Outcome::Installed(done));
        };
        // A platform's pass (D-184): the files it downloaded, or what is still missing.
        let at = dir.join(&entry.file);
        let mut parts =
            Downloads::open(&at, asset_len).map_err(|e| ClientError::of(at.display(), e))?;
        let mut mirrors = Mirrors::new(["downloads/"]);
        match c
            .store
            .install(&mut parts, &mut mirrors, &entry.file, entry, asset_len, set)
        {
            Err(FetchError::Needs(ranges)) => Ok(Outcome::Pending(Pending {
                pack: entry.path(),
                urls: c
                    .assets
                    .iter()
                    .map(|m| format!("{m}{}", entry.file))
                    .collect(),
                length: asset_len,
                dir: at,
                ranges,
            })),
            r => {
                let done = r?;
                // Committed: the downloaded files are no longer needed.
                let _ = std::fs::remove_dir_all(&at);
                Ok(Outcome::Installed(done))
            }
        }
    }
}
