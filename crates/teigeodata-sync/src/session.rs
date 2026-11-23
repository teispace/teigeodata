//! The store's client as one async core (D-374): a store, its trust anchor, its mirrors and a
//! transport, over an [`AsyncFs`] and an [`AsyncTransport`]. The native [`crate::client::Client`]
//! runs it over the store directory and a [`crate::fetch::Transport`] with
//! [`crate::meta::block_on`]; a browser worker runs it over the Origin Private File System and
//! `fetch`. One install, update, removal, repair and refresh path for every platform.
//!
//! ```text
//! config.json   the metadata and asset mirrors, from `init`
//! root.json     the trusted root `init` was given: the trust anchor
//! meta/         the verified catalog (`refresh`, D-179)
//! journal, packs/, staging/   the installed packs (`store`, D-171)
//! ```
//!
//! Nothing here reads a clock: each call that checks expiry takes `now` (Unix seconds), as a
//! browser's clock is the host's.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;
use std::io::{self, Write};

use serde_json::{Value, json};
use teigeodata_format::component::{Components, Selection};

use crate::catalog::{self, Entry};
use crate::fetch::{AsyncTransport, FetchError, Got, Mirrors, Ready, Transport};
use crate::meta::{self, Refreshed, StoreIo};
use crate::policy::{
    self, ClientError, Done, Outcome, PackState, Packs, Report, Result, Want, split, target,
};
use crate::store::{AsyncFs, AsyncStore, DeltaRecord, Health, Record, StoreFile};
use crate::trust::Trusted;

/// The store's configuration, written by [`init`].
pub const CONFIG: &str = "config.json";
/// The trust anchor [`init`] was given.
pub const ROOT: &str = "root.json";

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

/// A mirror base ends with `/`, so a file name joins it.
fn base(url: &str) -> String {
    if url.ends_with('/') {
        url.to_owned()
    } else {
        format!("{url}/")
    }
}

/// The configuration [`init`] writes: the metadata mirrors (`repo`) and the asset mirrors.
///
/// # Errors
/// [`ClientError::Refused`] if it cannot be encoded.
pub fn config_json(repo: &[String], assets: &[String]) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(&json!({ "repo": repo, "assets": assets }))
        .map_err(|e| ClientError::of(CONFIG, e))
}

/// The metadata and asset mirrors of a configuration, each ending with `/`.
///
/// # Errors
/// [`ClientError::Refused`] if it is not JSON.
pub fn parse_config(bytes: &[u8], what: impl core::fmt::Display) -> Result<Mirrored> {
    let config: Value = serde_json::from_slice(bytes).map_err(|e| ClientError::of(what, e))?;
    let list = |k: &str| -> Vec<String> {
        config
            .get(k)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(base).collect())
            .unwrap_or_default()
    };
    Ok(Mirrored {
        repo: list("repo"),
        assets: list("assets"),
    })
}

/// A store's mirrors, from its configuration.
#[derive(Clone, Debug, Default)]
pub struct Mirrored {
    /// The metadata mirrors.
    pub repo: Vec<String>,
    /// The asset mirrors.
    pub assets: Vec<String>,
}

/// Writes `data` to `path` in `fs` whole: to `path.tmp`, made durable, then moved over it.
async fn put<D: AsyncFs>(fs: &mut D, path: &str, data: &[u8]) -> io::Result<()> {
    let tmp = format!("{path}.tmp");
    let mut f = fs.open(&tmp).await?;
    f.set_len(0)?;
    f.write_all(data)?;
    f.sync()?;
    drop(f);
    fs.rename(&tmp, path).await?;
    fs.sync_dir("").await
}

/// Sets up a store in `fs`: its metadata and asset mirrors, and the trusted root that becomes
/// its trust anchor once it verifies by itself.
///
/// # Errors
/// [`ClientError::Refused`] if the root does not verify or the files cannot be written.
pub async fn init<D: AsyncFs>(
    fs: &mut D,
    repo: &[String],
    assets: &[String],
    trusted_root: &[u8],
    now: i64,
) -> Result<()> {
    Trusted::new(trusted_root, now).map_err(|e| ClientError::of("the trusted root", e))?;
    let config = config_json(repo, assets)?;
    for (name, data) in [(ROOT, trusted_root), (CONFIG, config.as_slice())] {
        put(fs, name, data)
            .await
            .map_err(|e| ClientError::of(name, e))?;
    }
    Ok(())
}

/// An open store: its packs, its catalog's trust anchor, its mirrors and its transport.
#[derive(Debug)]
pub struct Session<D: AsyncFs, N: AsyncTransport> {
    store: AsyncStore<D>,
    shipped: Vec<u8>,
    mirrors: Mirrored,
    net: N,
}

impl<D: AsyncFs, N: AsyncTransport> Session<D, N> {
    /// The store in `fs`, set up by [`init`], over the transport `net`. Also returns the packs
    /// found damaged when the store recovered: each needs [`Session::repair`].
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the store is not set up or cannot be read.
    pub async fn open(mut fs: D, net: N) -> Result<(Self, Vec<String>)> {
        let not_set_up = |e: &dyn core::fmt::Display| {
            ClientError::Refused(format!("{CONFIG}: {e} (set the store up with init first)"))
        };
        let config = fs
            .read(CONFIG)
            .await
            .map_err(|e| not_set_up(&e))?
            .ok_or_else(|| not_set_up(&"not found"))?;
        let mirrors = parse_config(&config, CONFIG)?;
        let shipped = fs
            .read(ROOT)
            .await
            .map_err(|e| ClientError::of(ROOT, e))?
            .ok_or_else(|| ClientError::of(ROOT, "not found"))?;
        let (store, recovered) = AsyncStore::open(fs)
            .await
            .map_err(|e| ClientError::of("the store", e))?;
        Ok((Self::new(store, shipped, mirrors, net), recovered.damaged))
    }

    /// A session over an open store, its trust anchor `shipped` and its mirrors.
    #[must_use]
    pub const fn new(store: AsyncStore<D>, shipped: Vec<u8>, mirrors: Mirrored, net: N) -> Self {
        Self {
            store,
            shipped,
            mirrors,
            net,
        }
    }

    /// The installed packs.
    #[must_use]
    pub fn packs(&self) -> &[Record] {
        self.store.packs()
    }

    /// The installed deltas (D-395).
    #[must_use]
    pub fn deltas(&self) -> &[DeltaRecord] {
        self.store.deltas()
    }

    /// The store.
    pub const fn store(&mut self) -> &mut AsyncStore<D> {
        &mut self.store
    }

    /// The transport, for a caller that watches it.
    pub const fn net(&mut self) -> &mut N {
        &mut self.net
    }

    /// The asset mirrors.
    #[must_use]
    pub fn assets(&self) -> &[String] {
        &self.mirrors.assets
    }

    /// The verified catalog, from the store alone.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if the stored metadata does not verify.
    pub async fn offline(&mut self, now: i64) -> Result<Trusted> {
        let mut none = Ready(Offline);
        let mut io = StoreIo {
            fs: self.store.fs_mut(),
            net: &mut none,
        };
        Ok(meta::load(&mut io, &self.shipped, now).await?)
    }

    /// The catalog refreshed from the mirrors, with the roles of `want` (country codes) and of
    /// every country whose pack is installed.
    ///
    /// # Errors
    /// [`ClientError::Network`] if no mirror answers, [`ClientError::Refused`] if what they
    /// send does not verify.
    pub async fn refresh(&mut self, want: &[String], now: i64) -> Result<Refreshed> {
        let mut want: Vec<String> = want.to_vec();
        for r in self.store.packs() {
            if let Some(cc) = catalog::country_of(&r.address) {
                want.push(String::from_utf8_lossy(&cc).into_owned());
            }
        }
        want.sort();
        want.dedup();
        let refs: Vec<&str> = want.iter().map(String::as_str).collect();
        let mut mirrors = Mirrors::new(self.mirrors.repo.clone());
        let mut io = StoreIo {
            fs: self.store.fs_mut(),
            net: &mut self.net,
        };
        Ok(meta::refresh(&mut io, &mut mirrors, &self.shipped, now, &refs).await?)
    }

    /// What the catalog says of `entry` against the store.
    #[must_use]
    pub fn state(&self, entry: &Entry) -> PackState {
        PackState::of(entry, &self.current())
    }

    /// The installed packs as they answer: each with its delta's release and head when one is
    /// installed above it (D-395, [`store::current`](crate::store::current)).
    #[must_use]
    pub fn current(&self) -> Vec<Record> {
        crate::store::current(self.store.packs(), self.store.deltas())
    }

    /// Installs the packs at `paths` (catalog paths) with `want` added to what is installed,
    /// dependencies first, pinning each when `pin`.
    ///
    /// # Errors
    /// [`ClientError::Network`] or [`ClientError::Refused`], from the first pack that fails;
    /// the packs before it stay installed.
    pub async fn install(
        &mut self,
        paths: &[String],
        want: &Want,
        pin: bool,
        now: i64,
    ) -> Result<Report> {
        policy::install(&mut self.at(now), paths, want, pin).await
    }

    /// Updates the packs at `paths` (every installed one when empty) to the catalog's
    /// releases, keeping the components and parts installed. Returns the packs updated.
    ///
    /// # Errors
    /// [`ClientError::Refused`] for a path not installed, else as [`Session::install`].
    pub async fn update(&mut self, paths: &[String], now: i64) -> Result<Vec<Done>> {
        policy::update(&mut self.at(now), paths).await
    }

    /// Removes the pack at `path` and the dependencies nothing else needs. Returns what was
    /// removed.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed or another pack needs it.
    pub async fn remove(&mut self, path: &str) -> Result<Vec<Record>> {
        let (a, c) = split(path);
        Ok(self.store.remove(a, c).await?)
    }

    /// Pins the pack at `path` (kept when room is made), or unpins it.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed.
    pub async fn pin(&mut self, path: &str, on: bool) -> Result<()> {
        let (a, c) = split(path);
        Ok(self.store.pin(a, c, on).await?)
    }

    /// Checks the pack at `path` in full and, unless it is whole, fetches what it lacks.
    ///
    /// # Errors
    /// [`ClientError::Network`] or [`ClientError::Refused`].
    pub async fn repair(&mut self, path: &str, now: i64) -> Result<Repaired> {
        let (a, c) = split(path);
        let health = self.store.verify(a, c, true).await?;
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
        let t = match self.offline(now).await {
            Ok(t) if target(&t, path).is_ok_and(|(e, _)| e.release == release) => t,
            _ => self.refresh(&[], now).await?.trusted,
        };
        let (entry, asset_len) = target(&t, path)?;
        let mut mirrors = Mirrors::new(self.mirrors.assets.clone());
        let done = self
            .store
            .repair(&mut self.net, &mut mirrors, &entry.file, &entry, asset_len)
            .await?;
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
    pub async fn status(&mut self, deep: bool, now: i64) -> Result<Status> {
        let catalog = self
            .offline(now)
            .await
            .ok()
            .and_then(|t| t.timestamp().map(|x| (x.common.version, x.common.expires)));
        let records: Vec<Record> = self.store.packs().to_vec();
        let mut packs = Vec::with_capacity(records.len());
        for r in records {
            let health = self.store.verify(&r.address, &r.component, deep).await?;
            packs.push((r, health));
        }
        Ok(Status { catalog, packs })
    }

    /// The session's operations for the policy at `now`.
    pub const fn at(&mut self, now: i64) -> At<'_, D, N> {
        At { session: self, now }
    }

    /// Installs `set` of `entry` from the asset mirrors: the policy's one install.
    async fn install_one(
        &mut self,
        entry: &Entry,
        asset_len: u64,
        set: Selection,
    ) -> Result<Outcome> {
        let mut mirrors = Mirrors::new(self.mirrors.assets.clone());
        self.net.expect(entry.size_of(set.set).wire);
        let done = self
            .store
            .install(
                &mut self.net,
                &mut mirrors,
                &entry.file,
                entry,
                asset_len,
                set,
            )
            .await?;
        Ok(Outcome::Installed(done))
    }

    /// Installs `delta` above its installed base from the session's mirrors (D-395).
    async fn install_delta(&mut self, delta: &catalog::Delta) -> Result<Outcome> {
        let mut mirrors = Mirrors::new(self.mirrors.assets.clone());
        let set = self
            .store
            .packs()
            .iter()
            .find(|r| r.key() == delta.entry.path() && r.release == delta.from)
            .map_or_else(|| Selection::from(Components::all()), Record::selection);
        self.net.expect(delta.entry.size_of(set).wire);
        let done = self
            .store
            .install_delta(&mut self.net, &mut mirrors, delta)
            .await?;
        Ok(Outcome::Installed(done))
    }
}

/// A session's operations for the policy ([`Packs`]) at one time.
#[derive(Debug)]
pub struct At<'a, D: AsyncFs, N: AsyncTransport> {
    /// The session.
    pub session: &'a mut Session<D, N>,
    /// Unix seconds, for the catalog's expiry.
    pub now: i64,
}

impl<D: AsyncFs, N: AsyncTransport> Packs for At<'_, D, N> {
    fn packs(&self) -> &[Record] {
        self.session.packs()
    }

    async fn refresh(&mut self, want: &[String]) -> Result<Trusted> {
        Ok(self.session.refresh(want, self.now).await?.trusted)
    }

    async fn offline(&mut self) -> Result<Trusted> {
        self.session.offline(self.now).await
    }

    async fn install(&mut self, entry: &Entry, asset_len: u64, set: Selection) -> Result<Outcome> {
        self.session.install_one(entry, asset_len, set).await
    }

    async fn mark(&mut self, address: &str, component: &str, asked: bool) -> Result<()> {
        Ok(self.session.store.mark(address, component, asked).await?)
    }

    async fn pin(&mut self, address: &str, component: &str, on: bool) -> Result<()> {
        Ok(self.session.store.pin(address, component, on).await?)
    }

    fn deltas(&self) -> &[DeltaRecord] {
        self.session.store.deltas()
    }

    async fn install_delta(&mut self, delta: &catalog::Delta) -> Result<Option<Outcome>> {
        self.session.install_delta(delta).await.map(Some)
    }
}

/// The transport of [`Session::offline`], which reads only the store.
pub(crate) struct Offline;

impl Transport for Offline {
    fn get(
        &mut self,
        _: &str,
        _: Range<u64>,
        _: &mut Vec<u8>,
    ) -> core::result::Result<Got, FetchError> {
        Err(FetchError::Response("offline"))
    }

    fn fetch(&mut self, _: &str, _: u64, _: &mut Vec<u8>) -> core::result::Result<(), FetchError> {
        Err(FetchError::Response("offline"))
    }
}
