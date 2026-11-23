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

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use std::path::{Path, PathBuf};

use teigeodata_format::Part;
use teigeodata_format::component::Selection;

use crate::catalog::{self, Entry};
use crate::fetch::{Downloads, FetchError, Mirrors, Ready, Transport, Ureq};
use crate::install::Installed;
use crate::meta::block_on;
use crate::policy::{self, Outcome, Packs};
pub use crate::policy::{
    ClientError, Done, PackState, Pending, Report, Result, Want, path_of, split,
};
use crate::refresh::{self, Refreshed};
use crate::session::{At, Session, config_json, parse_config};
pub use crate::session::{CONFIG, ROOT, Repaired, Status};
use crate::store::{AsyncStore, DeltaRecord, Dir, Record, Store, SyncFs};
use crate::trust::Trusted;

/// Seconds since the Unix epoch, for the catalog's expiry checks.
#[must_use]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The verified catalog stored at `root` and the packs its journal lists as they answer (each
/// with its delta's release and head, D-395), read without
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
    let (packs, deltas) =
        Store::read_journal(&mut dir).map_err(|e| ClientError::of(root.display(), e))?;
    Ok((trusted, crate::store::current(&packs, &deltas)))
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
    let config = config_json(repo, assets)?;
    for (name, data) in [(ROOT, trusted_root), (CONFIG, config.as_slice())] {
        let (path, tmp) = (root.join(name), root.join(format!("{name}.tmp")));
        std::fs::write(&tmp, data)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| ClientError::of(path.display(), e))?;
    }
    Ok(())
}

/// An open store: its packs, its catalog's trust anchor, its mirrors and its transport. The
/// [`Session`] core over the store directory and a [`Transport`], each future ready at once
/// (D-374).
#[derive(Debug)]
pub struct Client<N: Transport = Ureq> {
    root: PathBuf,
    session: Session<SyncFs<Dir>, Ready<N>>,
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
        let mirrors = parse_config(&config, at(CONFIG).display())?;
        let shipped =
            std::fs::read(at(ROOT)).map_err(|e| ClientError::of(at(ROOT).display(), e))?;
        let dir = Dir::new(root).map_err(|e| ClientError::of(root.display(), e))?;
        let (store, recovered) = block_on(AsyncStore::open(SyncFs(dir)))
            .map_err(|e| ClientError::of(root.display(), e))?;
        let client = Self {
            root: root.to_path_buf(),
            session: Session::new(store, shipped, mirrors, Ready(net)),
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
        self.session.packs()
    }

    /// Where an installed pack's file is.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the store's directory cannot be read.
    pub fn pack_path(&self, record: &Record) -> Result<PathBuf> {
        let dir = Dir::new(&self.root).map_err(|e| ClientError::of(self.root.display(), e))?;
        Ok(dir.pack_path(record))
    }

    /// The installed packs as they answer: each with its delta's release and head when one is
    /// installed above it (D-395).
    #[must_use]
    pub fn current(&self) -> Vec<Record> {
        self.session.current()
    }

    /// The installed deltas (D-395): each is opened as a layer, with its selection, beside its
    /// base.
    #[must_use]
    pub fn deltas(&self) -> &[DeltaRecord] {
        self.session.deltas()
    }

    /// Where an installed delta's file is.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the store's directory cannot be read.
    pub fn delta_path(&self, delta: &DeltaRecord) -> Result<PathBuf> {
        let dir = Dir::new(&self.root).map_err(|e| ClientError::of(self.root.display(), e))?;
        Ok(dir.delta_path(delta))
    }

    /// The transport, for a caller that watches it.
    pub fn net(&mut self) -> &mut N {
        &mut self.session.net().0
    }

    /// The verified catalog, from the store alone.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the stored metadata does not verify.
    pub fn offline(&mut self) -> Result<Trusted> {
        block_on(self.session.offline(now()))
    }

    /// The catalog refreshed from the mirrors, with the roles of `want` (country codes) and of
    /// every country whose pack is installed.
    ///
    /// # Errors
    /// [`ClientError::Network`] if no mirror answers, [`ClientError::Refused`] if what they
    /// send does not verify.
    pub fn refresh(&mut self, want: &[String]) -> Result<Refreshed> {
        block_on(self.session.refresh(want, now()))
    }

    /// What the catalog says of `entry` against the store.
    #[must_use]
    pub fn state(&self, entry: &Entry) -> PackState {
        self.session.state(entry)
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
        let now = now();
        let Some(dir) = downloads else {
            return block_on(self.session.install(paths, want, pin, now));
        };
        let mut pass = Downloaded {
            at: self.session.at(now),
            dir,
        };
        block_on(policy::install(&mut pass, paths, want, pin))
    }

    /// Updates the packs at `paths` (every installed one when empty) to the catalog's
    /// releases, keeping the components and parts installed. Returns the packs updated.
    ///
    /// # Errors
    /// [`ClientError::Refused`] for a path not installed, else as [`Client::install`].
    pub fn update(&mut self, paths: &[String]) -> Result<Vec<Done>> {
        block_on(self.session.update(paths, now()))
    }

    /// Removes the pack at `path` and the dependencies nothing else needs. Returns what was
    /// removed.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed or another pack needs it.
    pub fn remove(&mut self, path: &str) -> Result<Vec<Record>> {
        block_on(self.session.remove(path))
    }

    /// Pins the pack at `path` (kept when room is made), or unpins it.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed.
    pub fn pin(&mut self, path: &str, on: bool) -> Result<()> {
        block_on(self.session.pin(path, on))
    }

    /// Checks the pack at `path` in full and, unless it is whole, fetches what it lacks.
    ///
    /// # Errors
    /// [`ClientError::Network`] or [`ClientError::Refused`].
    pub fn repair(&mut self, path: &str) -> Result<Repaired> {
        block_on(self.session.repair(path, now()))
    }

    /// The catalog's freshness and each installed pack's health, from a full check when
    /// `deep`.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if a pack cannot be read.
    pub fn status(&mut self, deep: bool) -> Result<Status> {
        block_on(self.session.status(deep, now()))
    }
}

/// A platform's pass (D-184) for the policy: the session's operations, except that an install
/// reads the asset ranges a platform downloaded into a directory and reports what is missing.
struct Downloaded<'a, N: Transport> {
    at: At<'a, SyncFs<Dir>, Ready<N>>,
    dir: &'a Path,
}

impl<N: Transport> Packs for Downloaded<'_, N> {
    fn packs(&self) -> &[Record] {
        self.at.packs()
    }

    async fn refresh(&mut self, want: &[String]) -> Result<Trusted> {
        self.at.refresh(want).await
    }

    async fn offline(&mut self) -> Result<Trusted> {
        self.at.offline().await
    }

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "this driver is ready at once: its store calls run under block_on"
    )]
    async fn install(&mut self, entry: &Entry, asset_len: u64, set: Selection) -> Result<Outcome> {
        self.fetched(entry, asset_len, |store, t, mirrors| {
            block_on(store.install(t, mirrors, &entry.file, entry, asset_len, set))
        })
    }

    async fn mark(&mut self, address: &str, component: &str, asked: bool) -> Result<()> {
        self.at.mark(address, component, asked).await
    }

    async fn pin(&mut self, address: &str, component: &str, on: bool) -> Result<()> {
        self.at.pin(address, component, on).await
    }

    fn deltas(&self) -> &[DeltaRecord] {
        self.at.deltas()
    }

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "this driver is ready at once: its store calls run under block_on"
    )]
    async fn install_delta(&mut self, delta: &catalog::Delta) -> Result<Option<Outcome>> {
        self.fetched(&delta.entry, delta.length, |store, t, mirrors| {
            block_on(store.install_delta(t, mirrors, delta))
        })
        .map(Some)
    }
}

impl<N: Transport> Downloaded<'_, N> {
    /// Runs `op` (an install of `entry`, its asset `asset_len` bytes) over the platform's
    /// downloads of the asset (D-184): committed, the downloads go; short of ranges, they are
    /// what the platform still has to download.
    fn fetched(
        &mut self,
        entry: &Entry,
        asset_len: u64,
        op: impl FnOnce(
            &mut AsyncStore<SyncFs<Dir>>,
            &mut Ready<&mut Downloads>,
            &mut Mirrors,
        ) -> core::result::Result<Installed, FetchError>,
    ) -> Result<Outcome> {
        let at = self.dir.join(entry.name());
        let mut parts =
            Downloads::open(&at, asset_len).map_err(|e| ClientError::of(at.display(), e))?;
        let mut mirrors = Mirrors::new(["downloads/"]);
        let session = &mut *self.at.session;
        let r = op(session.store(), &mut Ready(&mut parts), &mut mirrors);
        match r {
            Err(FetchError::Needs(ranges)) => Ok(Outcome::Pending(Pending {
                pack: entry.path(),
                urls: session
                    .assets()
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
