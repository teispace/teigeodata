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
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use teigeodata_format::Part;
use teigeodata_format::component::{Component, Components, MAX_LANGUAGES, Parts, Selection};

use crate::catalog::{self, Entry};
use crate::fetch::{Downloads, FetchError, Mirrors, Transport, Ureq};
use crate::install::Installed;
use crate::refresh::{self, RefreshError, Refreshed};
use crate::store::{Dir, Health, Record, Store, StoreError};
use crate::trust::Trusted;

/// The store's configuration, written by [`init`].
pub const CONFIG: &str = "config.json";
/// The trust anchor [`init`] was given.
pub const ROOT: &str = "root.json";

/// A failed client call.
#[derive(Debug)]
pub enum ClientError {
    /// The network failed: the same call may work later.
    Network(String),
    /// The call cannot be done as asked: a refusal, a verification failure, a missing pack.
    Refused(String),
    /// The work was cancelled; the store is as it was before it.
    Cancelled,
}

impl ClientError {
    fn of(what: impl fmt::Display, e: impl fmt::Display) -> Self {
        Self::Refused(format!("{what}: {e}"))
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(m) | Self::Refused(m) => f.write_str(m),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<RefreshError> for ClientError {
    fn from(e: RefreshError) -> Self {
        match e {
            RefreshError::Network {
                error: FetchError::Cancelled,
                ..
            } => Self::Cancelled,
            RefreshError::Network { .. } => Self::Network(e.to_string()),
            e => Self::Refused(e.to_string()),
        }
    }
}

impl From<FetchError> for ClientError {
    fn from(e: FetchError) -> Self {
        if matches!(e, FetchError::Cancelled) {
            Self::Cancelled
        } else if matches!(e, FetchError::Transport(_) | FetchError::Status(_)) {
            Self::Network(e.to_string())
        } else {
            Self::Refused(e.to_string())
        }
    }
}

impl From<StoreError> for ClientError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Fetch(f) => f.into(),
            e => Self::Refused(e.to_string()),
        }
    }
}

/// A client call's result.
pub type Result<T> = core::result::Result<T, ClientError>;

/// Seconds since the Unix epoch, for the catalog's expiry checks.
#[must_use]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
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

/// The address and component of a catalog path.
#[must_use]
pub fn split(path: &str) -> (&str, &str) {
    let stem = path.strip_suffix(".tgd").unwrap_or(path);
    stem.split_once('.').unwrap_or((stem, "places"))
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

/// What an install takes of each pack it names: components (every one when empty) and, when
/// asked, only some languages and one outline part (D-253).
#[derive(Clone, Debug, Default)]
pub struct Want {
    /// The components named.
    pub components: Vec<Component>,
    /// The languages besides the base names; `None` for every one.
    pub languages: Option<Vec<Part>>,
    /// The deepest admin level and the tolerance in metres of the one outline part.
    pub outline: Option<(u8, u32)>,
}

impl Want {
    /// The components: those named, with names and boundaries when languages or an outline
    /// are asked for, closed by what they need; every one when none is named.
    #[must_use]
    pub fn set(&self) -> Components {
        if self.components.is_empty() {
            return Components::all();
        }
        let mut set = Components::of(&self.components);
        if self.languages.is_some() {
            set = set.union(Components::of(&[Component::Names]));
        }
        if self.outline.is_some() {
            set = set.union(Components::of(&[Component::Boundaries]));
        }
        set.closed()
    }
}

/// What the catalog says of a pack against the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackState {
    /// Not installed.
    Available,
    /// Installed at the catalog's release.
    Installed,
    /// Installed at another release.
    Update,
}

impl PackState {
    /// `available`, `installed` or `update`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Installed => "installed",
            Self::Update => "update",
        }
    }
}

/// One pack an install or update reached.
#[derive(Debug)]
pub struct Done {
    /// Its catalog path.
    pub path: String,
    /// The release now installed.
    pub release: String,
    /// What was fetched, reused and kept; `None` when it was installed already.
    pub installed: Option<Installed>,
}

/// A pack a platform still has to download (D-184): the ranges of its asset missing from
/// `dir`, each to be saved there under [`Downloads::name`].
#[derive(Debug)]
pub struct Pending {
    /// Its catalog path.
    pub pack: String,
    /// The asset's URL on each mirror.
    pub urls: Vec<String>,
    /// The asset's length.
    pub length: u64,
    /// The directory its ranges go into.
    pub dir: PathBuf,
    /// The ranges still missing.
    pub ranges: Vec<Range<u64>>,
}

/// What an install did.
#[derive(Debug, Default)]
pub struct Report {
    /// Each pack reached, dependencies first.
    pub done: Vec<Done>,
    /// What a platform's downloads still lack; empty without downloads.
    pub pending: Vec<Pending>,
    /// The network error when the catalog could not be refreshed and the stored one was used.
    pub stale_catalog: Option<String>,
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
        match self
            .store
            .packs()
            .iter()
            .find(|r| r.address == entry.address && r.component == entry.component)
        {
            None => PackState::Available,
            Some(r) if r.head == entry.head => PackState::Installed,
            Some(_) => PackState::Update,
        }
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
        let mut report = Report::default();
        let t = self.catalog_for(paths, &mut report)?;
        for p in paths {
            self.install_one(&t, p, Some(want), true, downloads, &mut report)?;
            if pin && self.store.packs().iter().any(|r| r.key() == *p) {
                let (a, c) = split(p);
                self.store.pin(a, c, true)?;
            }
        }
        Ok(report)
    }

    /// Updates the packs at `paths` (every installed one when empty) to the catalog's
    /// releases, keeping the components and parts installed. Returns the packs updated.
    ///
    /// # Errors
    /// [`ClientError::Refused`] for a path not installed, else as [`Client::install`].
    pub fn update(&mut self, paths: &[String]) -> Result<Vec<Done>> {
        let paths: Vec<String> = if paths.is_empty() {
            self.store.packs().iter().map(Record::key).collect()
        } else {
            paths.to_vec()
        };
        let t = self.refresh(&[])?.trusted;
        let mut report = Report::default();
        for p in &paths {
            if !self.store.packs().iter().any(|r| r.key() == *p) {
                return Err(ClientError::Refused(format!("{p}: not installed")));
            }
            let (entry, _) = target(&t, p)?;
            if self
                .store
                .packs()
                .iter()
                .any(|r| r.key() == *p && r.head != entry.head)
            {
                self.install_one(&t, p, None, true, None, &mut report)?;
            }
        }
        Ok(report.done)
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

    /// The catalog for installing `paths`: refreshed for their countries, or the stored one
    /// when the network fails (an install then fails at the network itself).
    fn catalog_for(&mut self, paths: &[String], report: &mut Report) -> Result<Trusted> {
        let want: Vec<String> = paths
            .iter()
            .filter_map(|p| catalog::country_of(p.split('.').next().unwrap_or_default()))
            .map(|cc| String::from_utf8_lossy(&cc).into_owned())
            .collect();
        match self.refresh(&want) {
            Ok(r) => Ok(r.trusted),
            Err(ClientError::Network(m)) => {
                report.stale_catalog = Some(m);
                self.offline()
            }
            Err(e) => Err(e),
        }
    }

    /// Installs `path` with `want` added to what is installed (or the installed selection, or
    /// everything), its dependencies first.
    fn install_one(
        &mut self,
        t: &Trusted,
        path: &str,
        want: Option<&Want>,
        asked: bool,
        downloads: Option<&Path>,
        report: &mut Report,
    ) -> Result<()> {
        let (entry, asset_len) = target(t, path)?;
        for dep in &entry.deps {
            if !self.store.packs().iter().any(|r| r.key() == *dep) {
                self.install_one(t, dep, None, false, downloads, report)?;
            }
        }
        let installed = self.store.packs().iter().find(|r| r.key() == path).cloned();
        let set = selection(&entry, want, installed.as_ref())?;
        if installed
            .as_ref()
            .is_some_and(|r| r.head == entry.head && r.selection() == set)
        {
            report.done.push(Done {
                path: path.to_owned(),
                release: entry.release.clone(),
                installed: None,
            });
            return Ok(());
        }
        let done = if let Some(dir) = downloads {
            // A platform's pass (D-184): the files it downloaded, or what is still missing.
            let at = dir.join(&entry.file);
            let mut parts =
                Downloads::open(&at, asset_len).map_err(|e| ClientError::of(at.display(), e))?;
            let mut mirrors = Mirrors::new(["downloads/"]);
            match self.store.install(
                &mut parts,
                &mut mirrors,
                &entry.file,
                &entry,
                asset_len,
                set,
            ) {
                Err(FetchError::Needs(ranges)) => {
                    report.pending.push(Pending {
                        pack: path.to_owned(),
                        urls: self
                            .assets
                            .iter()
                            .map(|m| format!("{m}{}", entry.file))
                            .collect(),
                        length: asset_len,
                        dir: at,
                        ranges,
                    });
                    return Ok(());
                }
                r => r?,
            }
        } else {
            let mut mirrors = Mirrors::new(self.assets.clone());
            self.net.expect(entry.size_of(set.set).wire);
            self.store.install(
                &mut self.net,
                &mut mirrors,
                &entry.file,
                &entry,
                asset_len,
                set,
            )?
        };
        if let Some(dir) = downloads {
            // Committed: the downloaded files are no longer needed.
            let _ = std::fs::remove_dir_all(dir.join(&entry.file));
        }
        if !asked && installed.is_none() {
            self.store.mark(&entry.address, &entry.component, false)?;
        }
        report.done.push(Done {
            path: path.to_owned(),
            release: entry.release.clone(),
            installed: Some(done),
        });
        Ok(())
    }
}

/// The catalog entry at `path` and its asset's length.
fn target(t: &Trusted, path: &str) -> Result<(Entry, u64)> {
    let (_, target) = t
        .trusted_targets()
        .find(|(p, _)| *p == path)
        .ok_or_else(|| ClientError::Refused(format!("{path}: not in the catalog")))?;
    let entry = Entry::from_target(path, target).map_err(|e| ClientError::of(path, e))?;
    Ok((entry, target.length))
}

/// What to install of `entry`: `want` added to what is `installed` (components added, the
/// parts it names replaced), or what is installed (an update, a dependency), or everything;
/// closed and resolved on the pack.
fn selection(entry: &Entry, want: Option<&Want>, installed: Option<&Record>) -> Result<Selection> {
    let set = match (want, installed) {
        (Some(w), r) => {
            let mut parts = r.map_or(Parts::ALL, |r| r.parts);
            if let Some(l) = &w.languages {
                parts = parts.languages(l).ok_or_else(|| {
                    ClientError::Refused(format!("at most {MAX_LANGUAGES} languages"))
                })?;
            }
            if let Some((level, t)) = w.outline {
                parts = parts.outline(entry.outline_for(level, t));
            }
            Selection {
                set: r.map_or(w.set(), |r| w.set().union(r.set)),
                parts,
            }
        }
        (None, Some(r)) => {
            // An update keeps the outline part, or, when the new release no longer has it,
            // takes the same levels at full resolution.
            let mut sel = r.selection();
            if let Some((level, _)) = r.parts.outline_part().and_then(Part::outline_levels)
                && !entry
                    .parts
                    .iter()
                    .any(|p| Some(p.part) == r.parts.outline_part())
            {
                sel.parts = sel.parts.outline(entry.outline_for(level, 0));
            }
            sel
        }
        (None, None) => Components::all().into(),
    };
    Ok(Selection {
        parts: entry.resolve(set.parts),
        ..set.closed()
    })
}
