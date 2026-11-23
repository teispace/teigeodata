//! The store policy as one core free of I/O (D-369): what an install and an update do,
//! written once over [`Packs`], the store's operations, and shared by the native client
//! ([`crate::client`]) and the browser store's worker.
//!
//! The core picks the catalog (refreshed for the countries asked, or the stored one when the
//! network fails), installs the dependencies a pack lacks before it, adds what is asked to
//! what is installed, and skips a pack already installed as asked. A driver's [`Packs`] does
//! the rest: where the bytes come from (mirrors, or a platform's downloads, D-184) and how a
//! pack is committed.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;
use std::path::PathBuf;

use teigeodata_format::Part;
use teigeodata_format::component::{Component, Components, MAX_LANGUAGES, Parts, Selection};

use crate::catalog::{self, Entry};
use crate::fetch::FetchError;
use crate::install::Installed;
use crate::meta::RefreshError;
use crate::store::{DeltaRecord, Record, StoreError};
use crate::trust::Trusted;

/// A failed client call.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// The network failed: the same call may work later.
    Network(String),
    /// The call cannot be done as asked: a refusal, a verification failure, a missing pack.
    Refused(String),
    /// The work was cancelled; the store is as it was before it.
    Cancelled,
}

impl ClientError {
    pub(crate) fn of(what: impl fmt::Display, e: impl fmt::Display) -> Self {
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

/// The address and component of a catalog path.
#[must_use]
pub fn split(path: &str) -> (&str, &str) {
    let stem = path.strip_suffix(".tgd").unwrap_or(path);
    stem.split_once('.').unwrap_or((stem, "places"))
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
#[non_exhaustive]
pub enum PackState {
    /// Not installed.
    Available,
    /// Installed at the catalog's release.
    Installed,
    /// Installed at another release.
    Update,
}

impl PackState {
    /// What `entry` is against the installed `packs`.
    #[must_use]
    pub fn of(entry: &Entry, packs: &[Record]) -> Self {
        match packs
            .iter()
            .find(|r| r.address == entry.address && r.component == entry.component)
        {
            None => Self::Available,
            Some(r) if r.head == entry.head => Self::Installed,
            Some(_) => Self::Update,
        }
    }

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
/// `dir`, each to be saved there under [`Downloads::name`](crate::fetch::Downloads::name).
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

/// What a driver's install did with one pack.
#[derive(Debug)]
pub enum Outcome {
    /// Installed and committed.
    Installed(Installed),
    /// Not installed: a platform still has to download these ranges (D-184).
    Pending(Pending),
}

/// The store's operations the policy needs.
#[allow(
    async_fn_in_trait,
    reason = "drivers are single-threaded (a browser worker) or ready at once (block_on); no Send bound is wanted"
)]
pub trait Packs {
    /// The installed packs.
    fn packs(&self) -> &[Record];
    /// The catalog refreshed with the roles of `want` (country codes) and of every country
    /// held.
    ///
    /// # Errors
    /// [`ClientError::Network`] if no mirror answers, else [`ClientError::Refused`].
    async fn refresh(&mut self, want: &[String]) -> Result<Trusted>;
    /// The stored catalog, with no network.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it does not verify.
    async fn offline(&mut self) -> Result<Trusted>;
    /// Installs `entry` (its asset `asset_len` bytes) with exactly `set`.
    ///
    /// # Errors
    /// The network's or the store's.
    async fn install(&mut self, entry: &Entry, asset_len: u64, set: Selection) -> Result<Outcome>;
    /// Marks an installed pack as asked for (kept) or as a dependency only.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed.
    async fn mark(&mut self, address: &str, component: &str, asked: bool) -> Result<()>;
    /// Pins an installed pack, or unpins it.
    ///
    /// # Errors
    /// [`ClientError::Refused`] if it is not installed.
    async fn pin(&mut self, address: &str, component: &str, on: bool) -> Result<()>;
    /// The installed deltas (D-395). None for a driver that installs none.
    fn deltas(&self) -> &[DeltaRecord] {
        &[]
    }
    /// Installs `delta` above its installed base (D-395); `None` for a driver that installs no
    /// deltas, which then updates by section.
    ///
    /// # Errors
    /// The network's or the store's.
    async fn install_delta(&mut self, delta: &catalog::Delta) -> Result<Option<Outcome>> {
        let _ = delta;
        Ok(None)
    }
}

/// A delta takes the place of an update by section only while it fetches at most this share of
/// what the update could fetch (D-395): past it, the update by section makes the newer release
/// the base again (compaction).
pub const DELTA_SHARE: u64 = 4;

/// The delta of `entry` (at `path` in `t`) above `base`, installed with `set`, when the catalog
/// lists one from the base's release and it costs at most a [`DELTA_SHARE`]th of the pack's
/// bytes for `set` (D-395).
fn delta_for(
    t: &Trusted,
    path: &str,
    entry: &Entry,
    base: &Record,
    set: Selection,
) -> Result<Option<catalog::Delta>> {
    let (_, target) = t
        .trusted_targets()
        .find(|(p, _)| *p == path)
        .ok_or_else(|| ClientError::Refused(format!("{path}: not in the catalog")))?;
    let deltas = catalog::Delta::list(path, target, entry).map_err(|e| ClientError::of(path, e))?;
    let whole = entry.size_of(set).wire;
    Ok(deltas.into_iter().find(|d| {
        d.from == base.release && d.entry.size_of(set).wire.saturating_mul(DELTA_SHARE) <= whole
    }))
}

/// Installs the packs at `paths` (catalog paths) with `want` added to what is installed,
/// dependencies first, pinning each when `pin`.
///
/// # Errors
/// [`ClientError::Network`] or [`ClientError::Refused`], from the first pack that fails;
/// the packs before it stay installed.
pub async fn install<P: Packs>(
    p: &mut P,
    paths: &[String],
    want: &Want,
    pin: bool,
) -> Result<Report> {
    let mut report = Report::default();
    let t = catalog_for(p, paths, &mut report).await?;
    if let Some(m) = &report.stale_catalog {
        // The refresh failed and the stored catalog lacks a pack asked for: the network,
        // not the pack, is why it cannot be installed.
        if let Some(path) = paths
            .iter()
            .find(|q| !t.trusted_targets().any(|(r, _)| r == q.as_str()))
        {
            return Err(ClientError::Network(format!(
                "{m}; {path} is not in the stored catalog"
            )));
        }
    }
    for path in paths {
        for dep in missing_deps(&t, path, p.packs())? {
            install_one(p, &t, &dep, None, false, &mut report).await?;
        }
        install_one(p, &t, path, Some(want), true, &mut report).await?;
        if pin && p.packs().iter().any(|r| r.key() == *path) {
            let (a, c) = split(path);
            p.pin(a, c, true).await?;
        }
    }
    Ok(report)
}

/// Updates the packs at `paths` (every installed one when empty) to the catalog's
/// releases, keeping the components and parts installed. Returns the packs updated.
///
/// # Errors
/// [`ClientError::Refused`] for a path not installed, else as [`install`].
pub async fn update<P: Packs>(p: &mut P, paths: &[String]) -> Result<Vec<Done>> {
    let paths: Vec<String> = if paths.is_empty() {
        p.packs().iter().map(Record::key).collect()
    } else {
        paths.to_vec()
    };
    let t = p.refresh(&[]).await?;
    let mut report = Report::default();
    for path in &paths {
        let Some(r) = p.packs().iter().find(|r| r.key() == *path) else {
            return Err(ClientError::Refused(format!("{path}: not installed")));
        };
        let (entry, _) = target(&t, path)?;
        if r.head != entry.head {
            install_one(p, &t, path, None, true, &mut report).await?;
        }
    }
    Ok(report.done)
}

/// The catalog for installing `paths`: refreshed for their countries, or the stored one
/// when the network fails (an install then fails at the network itself).
async fn catalog_for<P: Packs>(
    p: &mut P,
    paths: &[String],
    report: &mut Report,
) -> Result<Trusted> {
    let want: Vec<String> = paths
        .iter()
        .filter_map(|q| catalog::country_of(q.split('.').next().unwrap_or_default()))
        .map(|cc| String::from_utf8_lossy(&cc).into_owned())
        .collect();
    match p.refresh(&want).await {
        Ok(t) => Ok(t),
        Err(ClientError::Network(m)) => {
            report.stale_catalog = Some(m);
            p.offline().await
        }
        Err(e) => Err(e),
    }
}

/// The dependencies of `path` not installed, each after its own: depth first, each once, a
/// cycle cut where it closes.
fn missing_deps(t: &Trusted, path: &str, packs: &[Record]) -> Result<Vec<String>> {
    fn walk(
        t: &Trusted,
        path: &str,
        packs: &[Record],
        seen: &mut Vec<String>,
        out: &mut Vec<String>,
    ) -> Result<()> {
        let (entry, _) = target(t, path)?;
        for dep in &entry.deps {
            if packs.iter().any(|r| r.key() == *dep) || seen.contains(dep) {
                continue;
            }
            seen.push(dep.clone());
            walk(t, dep, packs, seen, out)?;
            out.push(dep.clone());
        }
        Ok(())
    }
    let mut seen = alloc::vec![path.to_owned()];
    let mut out = Vec::new();
    walk(t, path, packs, &mut seen, &mut out)?;
    Ok(out)
}

/// Installs `path` with `want` added to what is installed (or the installed selection, or
/// everything); `asked` when a person named it, not a dependency.
async fn install_one<P: Packs>(
    p: &mut P,
    t: &Trusted,
    path: &str,
    want: Option<&Want>,
    asked: bool,
    report: &mut Report,
) -> Result<()> {
    let (entry, asset_len) = target(t, path)?;
    let installed = p.packs().iter().find(|r| r.key() == path).cloned();
    let set = selection(&entry, want, installed.as_ref())?;
    // What the pack answers as: its delta's release when one is installed above it (D-395).
    let head = installed.as_ref().map(|r| {
        p.deltas()
            .iter()
            .find(|d| d.is_over(r))
            .map_or(r.head, |d| d.answers)
    });
    if installed.as_ref().is_some_and(|r| r.selection() == set) && head == Some(entry.head) {
        report.done.push(Done {
            path: path.to_owned(),
            release: entry.release.clone(),
            installed: None,
        });
        return Ok(());
    }
    // An update of what is installed takes a delta above the installed release when the
    // catalog lists one worth taking (D-395).
    if let Some(r) = installed.as_ref().filter(|r| r.selection() == set)
        && let Some(delta) = delta_for(t, path, &entry, r, set)?
        && let Some(outcome) = p.install_delta(&delta).await?
    {
        match outcome {
            Outcome::Installed(done) => report.done.push(Done {
                path: path.to_owned(),
                release: entry.release.clone(),
                installed: Some(done),
            }),
            Outcome::Pending(pending) => report.pending.push(pending),
        }
        return Ok(());
    }
    let done = match p.install(&entry, asset_len, set).await? {
        Outcome::Installed(done) => done,
        Outcome::Pending(pending) => {
            report.pending.push(pending);
            return Ok(());
        }
    };
    if !asked && installed.is_none() {
        p.mark(&entry.address, &entry.component, false).await?;
    }
    report.done.push(Done {
        path: path.to_owned(),
        release: entry.release.clone(),
        installed: Some(done),
    });
    Ok(())
}

/// The catalog entry at `path` and its asset's length.
pub(crate) fn target(t: &Trusted, path: &str) -> Result<(Entry, u64)> {
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
