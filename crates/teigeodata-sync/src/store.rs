//! The installed packs on disk (D-171): staging, the atomic install, the journal and crash
//! recovery.
//!
//! ```text
//! <root>/journal           the installed packs: the commit point
//! <root>/packs/<name>.tgd  immutable, one name per release and component set
//! <root>/staging/<name>.tgd.part
//! ```
//!
//! An install writes into staging (resumable, D-169), cuts the file to the pack's length,
//! syncs it, renames it into `packs/` and syncs that directory; then it writes the new journal
//! to `journal.tmp`, syncs it, renames it over `journal` and syncs the root. That last rename is
//! the commit: before it the old journal stands, after it the new one. A pack's name never
//! changes meaning, so a file a reader has mapped is never written over; the replaced version is
//! removed after the commit, or by the next recovery if it is still in use (Windows).
//!
//! [`Store::open`] recovers: it drops `journal.tmp`, removes files in `packs/` the journal does
//! not list (a crash between the rename and the commit, or a retired version), keeps staging for
//! resume, and reports listed packs whose file is missing or the wrong length.
//!
//! The storage manager (D-172) works on the same journal: each pack carries its dependencies,
//! whether it was asked for or installed as a dependency, a pin, and its last use.
//! [`plan_room`] picks least-recently-used packs to remove until an install fits the free
//! space and the quota, never a pinned pack, a kept one, or one a staying pack depends on;
//! [`Store::remove`] refuses a pack another one needs and removes dependencies left unneeded.
//! Every removal commits the journal first and deletes the files after, so a crash leaves at
//! most files that recovery removes.
//!
//! All file system calls go through [`Fs`], so tests can crash the store at every step.

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use std::io::{self, Read, Seek, Write};

use serde_json::{Map, Value};
use teigeodata_format::component::{Component, Components};
use teigeodata_format::{Kind, PackHead};

use crate::catalog::{self, Entry};
use crate::fetch::{Asset, FetchError, Mirrors, Transport, install_from, update_from};
use crate::install::Installed;

/// A file the store writes.
pub trait StoreFile: Read + Write + Seek {
    /// Sets the file's length.
    ///
    /// # Errors
    /// The file system's.
    fn set_len(&mut self, len: u64) -> io::Result<()>;
    /// Makes the file's data durable ([`std::fs::File::sync_all`]: `F_FULLFSYNC` on Apple
    /// platforms, `FlushFileBuffers` on Windows).
    ///
    /// # Errors
    /// The file system's.
    fn sync(&mut self) -> io::Result<()>;
    /// Frees the storage of what lies in `range` where the file system can, after which it
    /// reads as zeros; elsewhere it does nothing. The store calls it on the ranges a partial
    /// install never writes (D-189), so they take no space.
    ///
    /// # Errors
    /// The file system's.
    fn punch(&mut self, range: core::ops::Range<u64>) -> io::Result<()> {
        let _ = range;
        Ok(())
    }
}

/// The file system under a store's root. Paths are relative to the root and use `/`.
pub trait Fs {
    /// Its files.
    type File: StoreFile;
    /// Opens a file for reading and writing, creating it if missing, never truncating it.
    ///
    /// # Errors
    /// The file system's.
    fn open(&mut self, path: &str) -> io::Result<Self::File>;
    /// A whole file, or `None` if it does not exist.
    ///
    /// # Errors
    /// The file system's.
    fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>>;
    /// A file's length, or `None` if it does not exist.
    ///
    /// # Errors
    /// The file system's.
    fn len(&mut self, path: &str) -> io::Result<Option<u64>>;
    /// Renames `from` to `to`, replacing `to` atomically.
    ///
    /// # Errors
    /// The file system's.
    fn rename(&mut self, from: &str, to: &str) -> io::Result<()>;
    /// Removes a file; a missing one is not an error.
    ///
    /// # Errors
    /// The file system's.
    fn remove(&mut self, path: &str) -> io::Result<()>;
    /// Creates a directory if missing.
    ///
    /// # Errors
    /// The file system's.
    fn create_dir(&mut self, path: &str) -> io::Result<()>;
    /// Makes a directory's entries durable (`""` is the root).
    ///
    /// # Errors
    /// The file system's.
    fn sync_dir(&mut self, path: &str) -> io::Result<()>;
    /// The names of the files in a directory; none for a directory that does not exist.
    ///
    /// # Errors
    /// The file system's.
    fn list(&mut self, path: &str) -> io::Result<Vec<String>>;
}

/// One installed pack, as the journal records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Its address, component and release, as the catalog names them.
    pub address: String,
    /// The pack's component in the ADR-0010 sense.
    pub component: String,
    /// Its release.
    pub release: String,
    /// The components installed (closed under their needs).
    pub set: Components,
    /// Its file in `packs/`.
    pub file: String,
    /// Its length.
    pub pack_len: u64,
    /// The first 16 bytes of its head's BLAKE3, from the catalog.
    pub head: [u8; 16],
    /// The packs it depends on, as catalog paths (`address.component.tgd`), from the catalog.
    pub deps: Vec<String>,
    /// Asked for by the user or the app; `false` when installed as another pack's dependency,
    /// which is removed once no installed pack depends on it (apt's `auto`).
    pub asked: bool,
    /// Never evicted (Flatpak's pin); removed only when asked.
    pub pinned: bool,
    /// Its last use, in the caller's clock (seconds); 0 for never.
    pub used: u64,
}

impl Record {
    /// Its catalog path, `address.component.tgd`: what other packs' `deps` name.
    #[must_use]
    pub fn key(&self) -> String {
        catalog::path(&self.address, &self.component)
    }
}

/// Why a store operation failed.
#[derive(Debug)]
pub enum StoreError {
    /// The file system's error. The journal is untouched unless the commit itself succeeded.
    Io(io::Error),
    /// No pack of this address and component is installed.
    NotInstalled,
    /// The pack is a dependency of this installed pack: remove that one first.
    Needed {
        /// The dependant's catalog path.
        by: String,
    },
    /// The install does not fit even with every removable pack removed; nothing was removed.
    Short {
        /// Bytes the install needs.
        need: u64,
        /// The most bytes that could be had.
        room: u64,
    },
    /// The install refused the pack's name.
    Fetch(FetchError),
    /// A component could not be derived on the device; download it instead (D-213).
    Derive(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the store: {e}"),
            Self::NotInstalled => f.write_str("the pack is not installed"),
            Self::Needed { by } => write!(f, "the pack is needed by {by}"),
            Self::Short { need, room } => {
                write!(
                    f,
                    "the install needs {need} bytes and at most {room} can be had"
                )
            }
            Self::Fetch(e) => write!(f, "{e}"),
            Self::Derive(why) => write!(f, "cannot derive on the device: {why}"),
        }
    }
}

impl core::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Fetch(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<FetchError> for StoreError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::Io(e) => Self::Io(e),
            e => Self::Fetch(e),
        }
    }
}

/// What [`Store::verify`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    /// Every check passed.
    Good,
    /// The file is missing or not the length the journal records.
    Length,
    /// The head does not parse, or is not the head of the pack the journal records.
    Head,
    /// The footer is not the one the head implies.
    Footer,
    /// These installed sections do not match their hashes (deep checks only).
    Sections(Vec<Kind>),
}

/// Bytes read at a time when hashing or copying a pack.
const CHUNK: usize = 64 * 1024;

/// What an install asks of the disk, for [`plan_room`].
#[derive(Clone, Copy, Debug)]
pub struct Room<'a> {
    /// Bytes the install will write ([`Store::need`]).
    pub need: u64,
    /// Bytes free for this app now, from the platform (Android `getAllocatableBytes`, iOS
    /// `volumeAvailableCapacityForImportantUsage`, the web's `storage.estimate()`).
    pub available: u64,
    /// The most bytes the installed packs may take together, if the developer set a cap.
    pub quota: Option<u64>,
    /// Catalog paths never to remove: the pack being installed and its dependencies.
    pub keep: &'a [String],
}

impl Room<'_> {
    /// The bytes that can be written with `installed` bytes of packs left after removing
    /// `freed`.
    fn room(&self, installed: u64, freed: u64) -> u64 {
        let disk = self.available.saturating_add(freed);
        match self.quota {
            Some(q) => disk.min(q.saturating_sub(installed.saturating_sub(freed))),
            None => disk,
        }
    }
}

/// The packs to remove, in order, so that an install fits: least recently used first, a
/// dependency before a pack asked for at the same time, then the larger, then by path. Never
/// a pinned pack, one in `keep`, or one a staying pack depends on; a removed pack's own
/// dependencies may then go. Pure: it reads `packs` and decides.
///
/// # Errors
/// [`StoreError::Short`] when even removing every removable pack is not enough; nothing is to
/// be removed then.
pub fn plan_room(packs: &[Record], want: &Room<'_>) -> Result<Vec<usize>, StoreError> {
    let keys: Vec<String> = packs.iter().map(Record::key).collect();
    let at: BTreeMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (k.as_str(), i))
        .collect();
    // How many staying packs depend on each one.
    let mut needed = alloc::vec![0_usize; packs.len()];
    for r in packs {
        for d in &r.deps {
            if let Some(n) = at.get(d.as_str()).and_then(|&j| needed.get_mut(j)) {
                *n += 1;
            }
        }
    }
    let installed = packs
        .iter()
        .fold(0_u64, |a, r| a.saturating_add(r.pack_len));
    let mut gone = alloc::vec![false; packs.len()];
    let mut order = Vec::new();
    let mut freed = 0_u64;
    loop {
        if want.need <= want.room(installed, freed) {
            return Ok(order);
        }
        let pick = packs
            .iter()
            .enumerate()
            .filter(|&(i, r)| {
                !gone.get(i).copied().unwrap_or(true)
                    && needed.get(i) == Some(&0)
                    && !r.pinned
                    && keys.get(i).is_some_and(|k| !want.keep.contains(k))
            })
            .min_by(|(i, a), (j, b)| {
                (
                    a.used,
                    a.asked,
                    core::cmp::Reverse(a.pack_len),
                    keys.get(*i),
                )
                    .cmp(&(
                        b.used,
                        b.asked,
                        core::cmp::Reverse(b.pack_len),
                        keys.get(*j),
                    ))
            })
            .map(|(i, _)| i);
        let Some(i) = pick else {
            return Err(StoreError::Short {
                need: want.need,
                room: want.room(installed, freed),
            });
        };
        if let Some(g) = gone.get_mut(i) {
            *g = true;
        }
        order.push(i);
        if let Some(r) = packs.get(i) {
            freed = freed.saturating_add(r.pack_len);
            for d in &r.deps {
                if let Some(n) = at.get(d.as_str()).and_then(|&j| needed.get_mut(j)) {
                    *n = n.saturating_sub(1);
                }
            }
        }
    }
}

/// What [`Store::open`] found and did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    /// Files removed from `packs/`: not in the journal.
    pub removed: Vec<String>,
    /// Packs the journal lists whose file is missing or the wrong length: to repair.
    pub damaged: Vec<String>,
}

/// The installed packs under one root.
#[derive(Debug)]
pub struct Store<D: Fs> {
    fs: D,
    packs: Vec<Record>,
    /// Last-use times changed since the last commit.
    dirty: bool,
}

const JOURNAL: &str = "journal";
const JOURNAL_TMP: &str = "journal.tmp";
const PACKS: &str = "packs";
const STAGING: &str = "staging";
/// The journal's format version: 2 adds the marks, dependencies and last use (D-172); 1 is
/// read and upgraded on the next commit.
const VERSION: u64 = 2;

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

impl<D: Fs> Store<D> {
    /// Opens the store and recovers from any interrupted install.
    ///
    /// # Errors
    /// The file system's, or [`io::ErrorKind::InvalidData`] for a journal that does not parse:
    /// nothing is removed then, since the journal is the only record of what is installed.
    pub fn open(mut fs: D) -> io::Result<(Self, Recovered)> {
        for d in [PACKS, STAGING] {
            fs.create_dir(d)?;
        }
        fs.sync_dir("")?;
        fs.remove(JOURNAL_TMP)?;
        let packs = match fs.read(JOURNAL)? {
            Some(bytes) => parse(&bytes)?,
            None => Vec::new(),
        };
        let mut rec = Recovered::default();
        for name in fs.list(PACKS)? {
            if !packs.iter().any(|r| r.file == name) {
                fs.remove(&alloc::format!("{PACKS}/{name}"))?;
                rec.removed.push(name);
            }
        }
        if !rec.removed.is_empty() {
            fs.sync_dir(PACKS)?;
        }
        for r in &packs {
            if fs.len(&alloc::format!("{PACKS}/{}", r.file))? != Some(r.pack_len) {
                rec.damaged.push(r.file.clone());
            }
        }
        Ok((
            Self {
                fs,
                packs,
                dirty: false,
            },
            rec,
        ))
    }

    /// The installed packs.
    #[must_use]
    pub fn packs(&self) -> &[Record] {
        &self.packs
    }

    /// The path of an installed pack's file, relative to the root.
    #[must_use]
    pub fn path_of(record: &Record) -> String {
        alloc::format!("{PACKS}/{}", record.file)
    }

    /// The file system, for reading installed packs.
    pub fn fs(&mut self) -> &mut D {
        &mut self.fs
    }

    /// Installs `set` of the pack `entry` names from `mirrors` (the asset at `path` under each)
    /// and commits it, replacing any installed release or set of the same address and
    /// component. Staging is resumed if an earlier attempt left one, and a replaced release is
    /// read for every frame it holds unchanged (D-174), so an update fetches what changed. On success the new pack is
    /// in the journal and its file in `packs/`; the replaced one's file is removed, or left for
    /// the next recovery when it cannot be (in use).
    ///
    /// # Errors
    /// [`FetchError`]: the install's, [`FetchError::Older`] for a release older than the
    /// installed one, or [`FetchError::Io`] for the file system. The journal is untouched by any
    /// failure before the commit.
    pub fn install<T: Transport>(
        &mut self,
        transport: &mut T,
        mirrors: &mut Mirrors,
        path: &str,
        entry: &Entry,
        asset_len: u64,
        set: Components,
    ) -> Result<Installed, FetchError> {
        let set = set.closed();
        let file = pack_name(entry, set)?;
        // Never back to an older release (sync gate 1): the catalog is signed and fresh, but a
        // pack older than the installed one is refused here too.
        if let Some(old) = self.packs.iter().find(|r| {
            r.address == entry.address
                && r.component == entry.component
                && teigeodata_format::release_cmp(&entry.release, &r.release).is_lt()
        }) {
            return Err(FetchError::Older {
                installed: old.release.clone(),
            });
        }
        let staging = alloc::format!("{STAGING}/{file}.part");
        let mut f = self.fs.open(&staging)?;
        // An update reads the release it replaces and fetches only what changed (D-174).
        let replaced = self
            .packs
            .iter()
            .find(|r| {
                r.address == entry.address && r.component == entry.component && r.file != file
            })
            .map(Self::path_of);
        let mut previous = match replaced {
            Some(p) if self.fs.len(&p)?.is_some() => Some(self.fs.open(&p)?),
            _ => None,
        };
        let done = match previous.as_mut() {
            Some(old) => {
                let asset = Asset {
                    path,
                    entry,
                    len: asset_len,
                };
                update_from(transport, mirrors, asset, set, &mut f, old)?
            }
            None => install_from(transport, mirrors, path, entry, asset_len, set, &mut f)?,
        };
        drop(previous);
        f.set_len(done.pack_len)?;
        hollow(&mut f, done.pack_len, set)?;
        f.sync()?;
        drop(f);
        let target = alloc::format!("{PACKS}/{file}");
        self.fs.rename(&staging, &target)?;
        self.fs.sync_dir(PACKS)?;
        self.fs.sync_dir(STAGING)?;
        let mut packs = self.packs.clone();
        let old = packs
            .iter()
            .position(|r| r.address == entry.address && r.component == entry.component)
            .map(|i| packs.remove(i));
        // A new pack counts as used as recently as anything the store knows, so it is not the
        // first evicted; a replaced one keeps its marks.
        let latest = self.packs.iter().map(|r| r.used).max().unwrap_or(0);
        packs.push(Record {
            address: entry.address.clone(),
            component: entry.component.clone(),
            release: entry.release.clone(),
            set,
            file,
            pack_len: done.pack_len,
            head: entry.head,
            deps: entry.deps.clone(),
            asked: old.as_ref().is_none_or(|o| o.asked),
            pinned: old.as_ref().is_some_and(|o| o.pinned),
            used: old.as_ref().map_or(latest, |o| o.used.max(latest)),
        });
        packs.sort_by(|a, b| (&a.address, &a.component).cmp(&(&b.address, &b.component)));
        self.commit(packs)?;
        if let Some(old) = old {
            self.retire(&[old]);
        }
        Ok(done)
    }

    /// The bytes an install of `set` of `entry` will still write: the pack on disk less what
    /// staging already holds. The replaced release stays until the commit, so it frees nothing
    /// here (the disk peak of [download and delta §3.4]).
    ///
    /// # Errors
    /// [`StoreError::Io`], or [`StoreError::Fetch`] for an unsafe asset name.
    pub fn need(&mut self, entry: &Entry, set: Components) -> Result<u64, StoreError> {
        let staged = alloc::format!("{STAGING}/{}.part", pack_name(entry, set)?);
        let have = self.fs.len(&staged)?.unwrap_or(0);
        Ok(entry.size_of(set).disk.saturating_sub(have))
    }

    /// Removes least-recently-used packs until `set` of `entry` fits in `available` bytes and
    /// the `quota`, keeping the pack itself and its dependencies, and commits the removal.
    /// Returns what was removed (nothing when it already fits).
    ///
    /// # Errors
    /// [`StoreError::Short`] when it cannot fit (nothing removed), or [`StoreError::Io`].
    pub fn make_room(
        &mut self,
        entry: &Entry,
        set: Components,
        available: u64,
        quota: Option<u64>,
    ) -> Result<Vec<Record>, StoreError> {
        let need = self.need(entry, set)?;
        let mut keep = entry.deps.clone();
        keep.push(entry.path());
        let order = plan_room(
            &self.packs,
            &Room {
                need,
                available,
                quota,
                keep: &keep,
            },
        )?;
        self.drop_packs(&order)
    }

    /// Removes the pack of `address` and `component`, and then every pack installed as a
    /// dependency that no remaining pack needs and that is not pinned.
    ///
    /// # Errors
    /// [`StoreError::NotInstalled`], [`StoreError::Needed`] when another installed pack depends
    /// on it, or [`StoreError::Io`].
    pub fn remove(&mut self, address: &str, component: &str) -> Result<Vec<Record>, StoreError> {
        let i = self.find(address, component)?;
        let key = catalog::path(address, component);
        if let Some(by) = self.packs.iter().find(|r| r.deps.contains(&key)) {
            return Err(StoreError::Needed { by: by.key() });
        }
        let mut gone = alloc::vec![i];
        loop {
            let needed = |k: &String| {
                self.packs
                    .iter()
                    .enumerate()
                    .any(|(j, r)| !gone.contains(&j) && r.deps.contains(k))
            };
            let next =
                self.packs.iter().enumerate().find(|&(j, r)| {
                    !gone.contains(&j) && !r.asked && !r.pinned && !needed(&r.key())
                });
            match next {
                Some((j, _)) => gone.push(j),
                None => break,
            }
        }
        self.drop_packs(&gone)
    }

    /// Pins or unpins a pack. A pinned pack is never evicted.
    ///
    /// # Errors
    /// [`StoreError::NotInstalled`] or [`StoreError::Io`].
    pub fn pin(&mut self, address: &str, component: &str, pinned: bool) -> Result<(), StoreError> {
        self.set(address, component, |r| r.pinned = pinned)
    }

    /// Marks a pack as asked for (`true`) or as installed for another pack (`false`), which
    /// [`remove`](Self::remove) takes away once nothing needs it. A planner installing a
    /// dependency marks it after the install; a crash between leaves it asked, which only
    /// keeps it longer.
    ///
    /// # Errors
    /// [`StoreError::NotInstalled`] or [`StoreError::Io`].
    pub fn mark(&mut self, address: &str, component: &str, asked: bool) -> Result<(), StoreError> {
        self.set(address, component, |r| r.asked = asked)
    }

    /// Records a use of a pack at `now` (the caller's clock, seconds), in memory: it is saved
    /// with the next commit or by [`save`](Self::save). Returns whether the pack is installed.
    pub fn touch(&mut self, address: &str, component: &str, now: u64) -> bool {
        let Ok(i) = self.find(address, component) else {
            return false;
        };
        if let Some(r) = self.packs.get_mut(i)
            && now > r.used
        {
            r.used = now;
            self.dirty = true;
        }
        true
    }

    /// Saves the uses recorded by [`touch`](Self::touch), if any; a crash before it loses only
    /// the order of eviction.
    ///
    /// # Errors
    /// The file system's.
    pub fn save(&mut self) -> io::Result<()> {
        if self.dirty {
            self.commit(self.packs.clone())?;
        }
        Ok(())
    }

    /// Checks an installed pack against the journal, offline: its length, its head (parsed and
    /// equal to the recorded head hash, which the signed catalog gave), and its footer; with
    /// `deep`, every installed section against its BLAKE3 hash in the head, read a chunk at a
    /// time, so memory stays bounded whatever the pack's size. A section of a component not
    /// installed is a hole and is not read; `BLOCK_HASHES` is checked when an installed section
    /// is large enough to need it.
    ///
    /// # Errors
    /// [`StoreError::NotInstalled`] or [`StoreError::Io`].
    pub fn verify(
        &mut self,
        address: &str,
        component: &str,
        deep: bool,
    ) -> Result<Health, StoreError> {
        let r = self
            .packs
            .get(self.find(address, component)?)
            .cloned()
            .ok_or(StoreError::NotInstalled)?;
        let path = Self::path_of(&r);
        if self.fs.len(&path)? != Some(r.pack_len) {
            return Ok(Health::Length);
        }
        let mut f = self.fs.open(&path)?;
        let Some(head) = crate::fetch::read_head(&mut f, r.pack_len)? else {
            return Ok(Health::Head);
        };
        let Ok(h) = PackHead::parse(&head, r.pack_len) else {
            return Ok(Health::Head);
        };
        if h.digest().get(..16) != Some(&r.head[..]) {
            return Ok(Health::Head);
        }
        let mut footer = [0_u8; teigeodata_format::FOOTER_SIZE];
        f.seek(io::SeekFrom::Start(
            r.pack_len - teigeodata_format::FOOTER_SIZE as u64,
        ))?;
        f.read_exact(&mut footer)?;
        if footer != h.footer() {
            return Ok(Health::Footer);
        }
        if !deep {
            return Ok(Health::Good);
        }
        let wanted = held(&h, r.set);
        let mut bad = Vec::new();
        let mut buf = alloc::vec![0_u8; CHUNK];
        for s in h.sections() {
            if !wanted(s.kind) {
                continue;
            }
            f.seek(io::SeekFrom::Start(s.offset))?;
            let mut hasher = blake3::Hasher::new();
            let mut left = s.length;
            while left > 0 {
                let n = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
                let chunk = buf.get_mut(..n).ok_or_else(|| invalid("a chunk"))?;
                f.read_exact(chunk)?;
                hasher.update(chunk);
                left -= n as u64;
            }
            if hasher.finalize().as_bytes() != s.hash {
                bad.push(s.kind);
            }
        }
        Ok(if bad.is_empty() {
            Health::Good
        } else {
            Health::Sections(bad)
        })
    }

    /// Adds `component` to an installed pack by building it on the device from sections the
    /// pack already holds, instead of downloading it (D-213). `build` gets every installed
    /// section of the components it needs, each checked against its BLAKE3 hash in the head,
    /// and returns the component's one section; the result must match the hash the head
    /// lists for it, so a derived section is exactly the signed one. The new pack is written
    /// as an install writes one: staged under its new name (the old file is never written,
    /// so a reader that has it mapped is safe), synced, renamed into `packs/` and committed;
    /// the old file is retired. Already installed: nothing to do.
    ///
    /// # Errors
    /// [`StoreError::NotInstalled`]; [`StoreError::Derive`] when a needed component is not
    /// installed, the component is not one section, the result does not match its hash, or
    /// the pack would need `BLOCK_HASHES` it does not hold (download then); [`StoreError::Io`].
    pub fn derive(
        &mut self,
        address: &str,
        component: &str,
        add: Component,
        build: impl FnOnce(&[(Kind, u64, Vec<u8>)]) -> Result<Vec<u8>, String>,
    ) -> Result<(), StoreError> {
        let derive = |why: &str| StoreError::Derive(why.to_owned());
        let r = self
            .packs
            .get(self.find(address, component)?)
            .cloned()
            .ok_or(StoreError::NotInstalled)?;
        if r.set.contains(add) {
            return Ok(());
        }
        let needs = Components::of(add.needs()).closed();
        if !r.set.contains_all(needs) {
            return Err(derive("a component it is built from is not installed"));
        }
        let set = r.set.union(Components::of(&[add])).closed();
        let path = Self::path_of(&r);
        let mut old = self.fs.open(&path)?;
        let head = crate::fetch::read_head(&mut old, r.pack_len)?
            .ok_or_else(|| derive("the pack's head does not read"))?;
        let h = PackHead::parse(&head, r.pack_len)
            .map_err(|e| StoreError::Derive(alloc::format!("{e}")))?;
        let (was, now) = (held(&h, r.set), held(&h, set));
        if now(Kind::BLOCK_HASHES) && !was(Kind::BLOCK_HASHES) {
            return Err(derive(
                "the pack would need BLOCK_HASHES, which only a download brings",
            ));
        }
        let mut target = None;
        let mut inputs = Vec::new();
        for s in h.sections() {
            match Component::of(s.kind) {
                Some(c) if c == add => {
                    if target.replace(s).is_some() {
                        return Err(derive("the component is more than one section"));
                    }
                }
                Some(c) if needs.contains(c) => {
                    let bytes = read_checked(&mut old, &s)?
                        .ok_or_else(|| derive("an installed section does not match its hash"))?;
                    inputs.push((s.kind, s.item_count, bytes));
                }
                _ => {}
            }
        }
        let target = target.ok_or_else(|| derive("the pack has no such section"))?;
        let body = build(&inputs).map_err(StoreError::Derive)?;
        if body.len() as u64 != target.length || blake3::hash(&body).as_bytes() != target.hash {
            return Err(derive("the derived section does not match the pack's hash"));
        }
        drop(inputs);
        let file = name_with(&r.file, r.set, set)?;
        let staging = alloc::format!("{STAGING}/{file}.part");
        let mut f = self.fs.open(&staging)?;
        f.set_len(0)?;
        f.set_len(r.pack_len)?;
        // The head, every held section and the footer, a chunk at a time; then the new one.
        let first = h.sections().next().map_or(r.pack_len, |s| s.offset);
        let footer = r
            .pack_len
            .saturating_sub(teigeodata_format::FOOTER_SIZE as u64);
        copy_range(&mut old, &mut f, 0, first)?;
        for s in h.sections().filter(|s| was(s.kind) && s.length > 0) {
            copy_range(&mut old, &mut f, s.offset, s.length)?;
        }
        copy_range(&mut old, &mut f, footer, r.pack_len - footer)?;
        drop(old);
        f.seek(io::SeekFrom::Start(target.offset))?;
        f.write_all(&body)?;
        hollow(&mut f, r.pack_len, set)?;
        f.sync()?;
        drop(f);
        self.fs
            .rename(&staging, &alloc::format!("{PACKS}/{file}"))?;
        self.fs.sync_dir(PACKS)?;
        self.fs.sync_dir(STAGING)?;
        let mut packs = self.packs.clone();
        if let Some(p) = packs.iter_mut().find(|p| p.file == r.file) {
            p.set = set;
            p.file = file;
        }
        self.commit(packs)?;
        self.retire(&[r]);
        Ok(())
    }

    /// Repairs an installed pack from `mirrors`: copies it into staging, where the resume path
    /// (D-169) keeps every frame that checks against the signed catalog and fetches only the
    /// rest, then renames the result over the same name. The journal does not change (same
    /// release, same bytes meant), so a crash leaves the damaged or the repaired file, and a
    /// second call resumes. `entry` must name the installed release. On Windows a mapped file
    /// cannot be renamed over: drop the layer first.
    ///
    /// # Errors
    /// [`StoreError::NotInstalled`]; [`StoreError::Fetch`] when `entry` is another release or
    /// the install fails; [`StoreError::Io`].
    pub fn repair<T: Transport>(
        &mut self,
        transport: &mut T,
        mirrors: &mut Mirrors,
        path: &str,
        entry: &Entry,
        asset_len: u64,
    ) -> Result<Installed, StoreError> {
        let r = self
            .packs
            .get(self.find(&entry.address, &entry.component)?)
            .cloned()
            .ok_or(StoreError::NotInstalled)?;
        if r.release != entry.release || r.head != entry.head {
            return Err(StoreError::Fetch(FetchError::Response(
                "the catalog names another release: install it instead",
            )));
        }
        let target = Self::path_of(&r);
        let staging = alloc::format!("{STAGING}/{}.part", r.file);
        let mut f = self.fs.open(&staging)?;
        if f.seek(io::SeekFrom::End(0))? == 0 {
            // Start from the installed bytes; whatever does not check is fetched again.
            let mut from = self.fs.open(&target)?;
            let mut buf = alloc::vec![0_u8; CHUNK];
            loop {
                let n = from.read(&mut buf)?;
                let Some(chunk) = buf.get(..n).filter(|c| !c.is_empty()) else {
                    break;
                };
                f.write_all(chunk)?;
            }
        }
        let done = install_from(transport, mirrors, path, entry, asset_len, r.set, &mut f)?;
        f.set_len(done.pack_len)?;
        hollow(&mut f, done.pack_len, r.set)?;
        f.sync()?;
        drop(f);
        self.fs.rename(&staging, &target)?;
        self.fs.sync_dir(PACKS)?;
        self.fs.sync_dir(STAGING)?;
        Ok(done)
    }

    fn find(&self, address: &str, component: &str) -> Result<usize, StoreError> {
        self.packs
            .iter()
            .position(|r| r.address == address && r.component == component)
            .ok_or(StoreError::NotInstalled)
    }

    fn set(
        &mut self,
        address: &str,
        component: &str,
        change: impl FnOnce(&mut Record),
    ) -> Result<(), StoreError> {
        let i = self.find(address, component)?;
        let mut packs = self.packs.clone();
        if let Some(r) = packs.get_mut(i) {
            let was = r.clone();
            change(r);
            if *r == was {
                return Ok(());
            }
        }
        Ok(self.commit(packs)?)
    }

    /// Commits the journal without the packs at `indices`, then deletes their files.
    fn drop_packs(&mut self, indices: &[usize]) -> Result<Vec<Record>, StoreError> {
        if indices.is_empty() {
            return Ok(Vec::new());
        }
        let (gone, stay): (Vec<_>, Vec<_>) = self
            .packs
            .iter()
            .cloned()
            .enumerate()
            .partition(|(i, _)| indices.contains(i));
        self.commit(stay.into_iter().map(|(_, r)| r).collect())?;
        let gone: Vec<Record> = gone.into_iter().map(|(_, r)| r).collect();
        self.retire(&gone);
        Ok(gone)
    }

    /// Deletes the files of packs no longer in the journal. Best effort: a mapped file on
    /// Windows cannot go yet, and recovery removes it later.
    fn retire(&mut self, old: &[Record]) {
        let mut any = false;
        for o in old {
            if self.packs.iter().all(|r| r.file != o.file) {
                any |= self.fs.remove(&Self::path_of(o)).is_ok();
            }
        }
        if any {
            let _ = self.fs.sync_dir(PACKS);
        }
    }

    /// Writes `packs` as the journal (temp file, sync, rename, sync the root) and makes them
    /// the store's.
    fn commit(&mut self, packs: Vec<Record>) -> io::Result<()> {
        let bytes = render(&packs);
        let mut t = self.fs.open(JOURNAL_TMP)?;
        t.set_len(0)?;
        t.write_all(&bytes)?;
        t.sync()?;
        drop(t);
        self.fs.rename(JOURNAL_TMP, JOURNAL)?;
        self.fs.sync_dir("")?;
        self.packs = packs;
        self.dirty = false;
        Ok(())
    }
}

/// A section's bytes, read from `f` and checked against its hash; `None` when they differ.
fn read_checked<F: Read + Seek>(
    f: &mut F,
    s: &teigeodata_format::SectionEntry<'_>,
) -> io::Result<Option<Vec<u8>>> {
    let len = usize::try_from(s.length).map_err(|_| invalid("a section's length"))?;
    let mut bytes = alloc::vec![0_u8; len];
    f.seek(io::SeekFrom::Start(s.offset))?;
    f.read_exact(&mut bytes)?;
    Ok((blake3::hash(&bytes).as_bytes() == s.hash).then_some(bytes))
}

/// Copies `len` bytes at `at` from `from` to the same place in `to`, a chunk at a time.
fn copy_range<A: Read + Seek, B: Write + Seek>(
    from: &mut A,
    to: &mut B,
    at: u64,
    len: u64,
) -> io::Result<()> {
    from.seek(io::SeekFrom::Start(at))?;
    to.seek(io::SeekFrom::Start(at))?;
    let mut buf = alloc::vec![0_u8; CHUNK];
    let mut left = len;
    while left > 0 {
        let n = usize::try_from(left).map_or(CHUNK, |l| l.min(CHUNK));
        let chunk = buf.get_mut(..n).ok_or_else(|| invalid("a chunk"))?;
        from.read_exact(chunk)?;
        to.write_all(chunk)?;
        left -= n as u64;
    }
    Ok(())
}

/// The installed file name of `file` (installed with `old`) when it holds `new` instead.
fn name_with(file: &str, old: Components, new: Components) -> Result<String, StoreError> {
    let base = file
        .strip_suffix(&alloc::format!(".{}.tgd", set_tag(old)))
        .ok_or_else(|| StoreError::Derive("an installed file name without its set".into()))?;
    Ok(alloc::format!("{base}.{}.tgd", set_tag(new)))
}

/// A component set as file names spell it: `all`, or the names joined by `+`.
fn set_tag(set: Components) -> String {
    let set = set.closed();
    if set == Components::all().closed() {
        "all".to_owned()
    } else {
        set.iter()
            .map(Component::name)
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// The installed file's name: the asset's, with the component set in place of `.zst`.
///
/// # Errors
/// [`FetchError::Response`] for a name that is not plain lowercase ASCII (the catalog is signed,
/// but a path is still checked before it touches the disk).
pub fn pack_name(entry: &Entry, set: Components) -> Result<String, FetchError> {
    let base = entry
        .file
        .strip_suffix(".tgd.zst")
        .ok_or(FetchError::Response("an asset name not ending .tgd.zst"))?;
    let name = alloc::format!("{base}.{}.tgd", set_tag(set));
    let plain = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-._".contains(c);
    if name.starts_with('.') || name.contains("..") || !name.chars().all(plain) {
        return Err(FetchError::Response("an unsafe pack name"));
    }
    Ok(name)
}

fn render(packs: &[Record]) -> Vec<u8> {
    let list: Vec<Value> = packs
        .iter()
        .map(|r| {
            let mut m = Map::new();
            m.insert("address".into(), r.address.clone().into());
            m.insert("component".into(), r.component.clone().into());
            m.insert("release".into(), r.release.clone().into());
            let set: Vec<Value> = r.set.iter().map(|c| c.name().into()).collect();
            m.insert("set".into(), set.into());
            m.insert("file".into(), r.file.clone().into());
            m.insert("pack_len".into(), r.pack_len.into());
            let head = crate::trust::lower_hex(&r.head);
            m.insert("head".into(), head.into());
            let deps: Vec<Value> = r.deps.iter().map(|d| d.clone().into()).collect();
            m.insert("deps".into(), deps.into());
            m.insert("asked".into(), r.asked.into());
            m.insert("pinned".into(), r.pinned.into());
            m.insert("used".into(), r.used.into());
            Value::Object(m)
        })
        .collect();
    let mut top = Map::new();
    top.insert("version".into(), VERSION.into());
    top.insert("packs".into(), list.into());
    let mut out = serde_json::to_vec(&Value::Object(top)).unwrap_or_default();
    out.push(b'\n');
    out
}

fn parse(bytes: &[u8]) -> io::Result<Vec<Record>> {
    let bad = || invalid("the journal does not parse");
    let v: Value = serde_json::from_slice(bytes).map_err(|_| bad())?;
    let version = v.get("version").and_then(Value::as_u64);
    if !matches!(version, Some(1 | VERSION)) {
        return Err(invalid("the journal's version is unknown"));
    }
    let v1 = version == Some(1);
    let text = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).map(String::from);
    let mut out = Vec::new();
    for r in v.get("packs").and_then(Value::as_array).ok_or_else(bad)? {
        let mut set = Components::default();
        for c in r.get("set").and_then(Value::as_array).ok_or_else(bad)? {
            let c = c.as_str().and_then(Component::from_name).ok_or_else(bad)?;
            set = set.union(Components::of(&[c]));
        }
        let hex = text(r, "head").ok_or_else(bad)?;
        let mut head = [0_u8; 16];
        if hex.len() != 32 {
            return Err(bad());
        }
        for (i, b) in head.iter_mut().enumerate() {
            *b = hex
                .get(2 * i..2 * i + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(bad)?;
        }
        // Version 1 had no marks: a pack asked for, unpinned, never used.
        let (deps, asked, pinned, used) = if v1 {
            (Vec::new(), true, false, 0)
        } else {
            let mut deps = Vec::new();
            for d in r.get("deps").and_then(Value::as_array).ok_or_else(bad)? {
                deps.push(d.as_str().ok_or_else(bad)?.to_owned());
            }
            (
                deps,
                r.get("asked").and_then(Value::as_bool).ok_or_else(bad)?,
                r.get("pinned").and_then(Value::as_bool).ok_or_else(bad)?,
                r.get("used").and_then(Value::as_u64).ok_or_else(bad)?,
            )
        };
        out.push(Record {
            address: text(r, "address").ok_or_else(bad)?,
            component: text(r, "component").ok_or_else(bad)?,
            release: text(r, "release").ok_or_else(bad)?,
            set,
            file: text(r, "file")
                .filter(|f| !f.contains('/') && !f.starts_with('.'))
                .ok_or_else(bad)?,
            pack_len: r.get("pack_len").and_then(Value::as_u64).ok_or_else(bad)?,
            head,
            deps,
            asked,
            pinned,
            used,
        });
    }
    Ok(out)
}

/// The real file system under a root directory.
#[derive(Clone, Debug)]
pub struct Dir {
    root: std::path::PathBuf,
}

impl Dir {
    /// The store's root; created if missing.
    ///
    /// # Errors
    /// The file system's.
    pub fn new(root: impl Into<std::path::PathBuf>) -> io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// The file of an installed pack.
    #[must_use]
    pub fn pack_path(&self, record: &Record) -> std::path::PathBuf {
        self.at(PACKS).join(&record.file)
    }

    /// The directory of the installed packs.
    #[must_use]
    pub fn packs_dir(&self) -> std::path::PathBuf {
        self.at(PACKS)
    }

    fn at(&self, path: &str) -> std::path::PathBuf {
        path.split('/')
            .filter(|p| !p.is_empty())
            .fold(self.root.clone(), |p, s| p.join(s))
    }
}

impl StoreFile for std::fs::File {
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        std::fs::File::set_len(self, len)
    }
    fn sync(&mut self) -> io::Result<()> {
        self.sync_all()
    }
    fn punch(&mut self, range: core::ops::Range<u64>) -> io::Result<()> {
        crate::holes::punch(self, range.start, range.end).map(drop)
    }
}

/// Whether an install of `set` holds section `kind` of the pack `h` heads: its components'
/// sections, sections of no component, and `BLOCK_HASHES` when it covers one of them.
fn held(h: &PackHead<'_>, set: Components) -> impl Fn(Kind) -> bool + use<> {
    let installed = move |kind: Kind| Component::of(kind).is_none_or(|c| set.contains(c));
    let blocked = h.sections().any(|s| {
        installed(s.kind)
            && s.kind != Kind::BLOCK_HASHES
            && s.length >= teigeodata_format::blocks::MIN_COVERED
    });
    move |kind| {
        if kind == Kind::BLOCK_HASHES {
            blocked
        } else {
            installed(kind)
        }
    }
}

/// Punches every range of the staged pack that an install of `set` never writes: the sections
/// it does not hold and the gaps around them, never the head, a held section or the footer
/// (D-189). A file whose head does not read is left as it is: the install checked it already.
fn hollow<F: StoreFile>(f: &mut F, pack_len: u64, set: Components) -> io::Result<()> {
    let Some(head) = crate::fetch::read_head(f, pack_len)? else {
        return Ok(());
    };
    let Ok(h) = PackHead::parse(&head, pack_len) else {
        return Ok(());
    };
    let wanted = held(&h, set);
    let Some(mut at) = h.sections().next().map(|s| s.offset) else {
        return Ok(());
    };
    for s in h.sections().filter(|s| wanted(s.kind) && s.length > 0) {
        if s.offset > at {
            f.punch(at..s.offset)?;
        }
        at = at.max(s.offset.saturating_add(s.length));
    }
    let footer = pack_len.saturating_sub(teigeodata_format::FOOTER_SIZE as u64);
    if footer > at {
        f.punch(at..footer)?;
    }
    Ok(())
}

impl Fs for Dir {
    type File = std::fs::File;

    fn open(&mut self, path: &str) -> io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.at(path))
    }

    fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.at(path)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn len(&mut self, path: &str) -> io::Result<Option<u64>> {
        match std::fs::metadata(self.at(path)) {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        std::fs::rename(self.at(from), self.at(to))
    }

    fn remove(&mut self, path: &str) -> io::Result<()> {
        match std::fs::remove_file(self.at(path)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }

    fn create_dir(&mut self, path: &str) -> io::Result<()> {
        std::fs::create_dir_all(self.at(path))
    }

    fn sync_dir(&mut self, path: &str) -> io::Result<()> {
        // A directory is synced through a handle on it on Unix. Windows cannot open one this
        // way; its file system journal makes the rename durable there.
        #[cfg(unix)]
        std::fs::File::open(self.at(path))?.sync_all()?;
        #[cfg(not(unix))]
        let _ = path;
        Ok(())
    }

    fn list(&mut self, path: &str) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(self.at(path)) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for e in entries {
            let e = e?;
            if e.file_type()?.is_file()
                && let Some(n) = e.file_name().to_str()
            {
                out.push(n.to_owned());
            }
        }
        out.sort_unstable();
        Ok(out)
    }
}

#[cfg(all(test, feature = "write"))]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "tests: small fixed data, failures panic with their message"
)]
mod derive_tests {
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::string::String;
    use alloc::vec::Vec;

    use teigeodata_format::component::{Component, Components};
    use teigeodata_format::names::{self, NameRecord, Names};
    use teigeodata_format::search::{self, Search};
    use teigeodata_format::{Encoding, Kind, Pack, Section, Writer};

    use super::{Dir, JOURNAL, PACKS, Record, Store, StoreError, render, set_tag};

    /// Every suffix of 3 or more bytes of a lowercase ASCII name: the test's key rule.
    fn keys(text: &str, out: &mut dyn FnMut(&str)) {
        let t = text.to_ascii_lowercase();
        for at in 1..t.len().saturating_sub(2) {
            if let Some(k) = t.get(at..) {
                out(k);
            }
        }
    }

    fn build(inputs: &[(Kind, u64, Vec<u8>)]) -> Result<Vec<u8>, String> {
        let get = |k: Kind| inputs.iter().find(|i| i.0 == k).ok_or("missing");
        let (n, s) = (get(Kind::NAMES)?, get(Kind::SEARCH)?);
        let names = Names::open(&Section::unpacked(Kind::NAMES, n.1, &n.2))
            .map_err(|e| alloc::format!("{e}"))?;
        let search = Search::open(&Section::unpacked(Kind::SEARCH, s.1, &s.2))
            .map_err(|e| alloc::format!("{e}"))?;
        let imp: Vec<u8> = (0..names.len())
            .map(|r| search.importance(r as u32))
            .collect();
        search::substring_from_names(&names, &imp, &keys).map_err(|e| alloc::format!("{e}"))
    }

    /// A pack of NAMES, SEARCH and SUBSTRING over a few places.
    fn pack() -> Vec<u8> {
        let texts = [
            "Lahachowk",
            "Pokhara",
            "Kathmandu",
            "Biratnagar",
            "Janakpur",
        ];
        let records: Vec<[NameRecord<'_>; 1]> = texts
            .iter()
            .map(|t| {
                [NameRecord {
                    text: t,
                    ..NameRecord::default()
                }]
            })
            .collect();
        let refs: Vec<&[NameRecord<'_>]> = records.iter().map(<[_; 1]>::as_slice).collect();
        let names_body = names::encode(&refs).unwrap();
        let mut map: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
        for (r, t) in texts.iter().enumerate() {
            let mut k = alloc::vec![1_u8];
            k.extend_from_slice(t.to_ascii_lowercase().as_bytes());
            map.entry(k).or_default().insert(r as u32);
        }
        let imp = [5_u8, 9, 30, 12, 7];
        let search_body = search::encode(&map, &imp).unwrap();
        let n = texts.len() as u64;
        let sub = build(&[
            (Kind::NAMES, n, names_body.clone()),
            (Kind::SEARCH, n, search_body.clone()),
        ])
        .unwrap();
        Writer::new("zz", "places", "2026.11.0")
            .unwrap()
            .pack(
                &[
                    (Kind::NAMES, Encoding::TABLE, n, &names_body),
                    (Kind::SEARCH, Encoding::TABLE, n, &search_body),
                    (Kind::SUBSTRING, Encoding::TABLE, n, &sub),
                ],
                None,
            )
            .unwrap()
    }

    /// A store holding `pack` installed without SUBSTRING (its bytes zero, as a hole reads).
    fn store(tag: &str, full: &[u8]) -> (std::path::PathBuf, Store<Dir>, Record) {
        let root =
            std::env::temp_dir().join(alloc::format!("tgd-derive-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let set = Components::of(&[Component::Search]).closed();
        let file = alloc::format!("zz.places.2026.11.0.{}.tgd", set_tag(set));
        let mut bytes = full.to_vec();
        let p = Pack::open(full).unwrap();
        let s = p
            .head()
            .sections()
            .find(|s| s.kind == Kind::SUBSTRING)
            .unwrap();
        bytes[s.offset as usize..(s.offset + s.length) as usize].fill(0);
        std::fs::create_dir_all(root.join(PACKS)).unwrap();
        std::fs::write(root.join(PACKS).join(&file), &bytes).unwrap();
        let r = Record {
            address: "zz".into(),
            component: "places".into(),
            release: "2026.11.0".into(),
            set,
            file,
            pack_len: bytes.len() as u64,
            head: [0; 16],
            deps: alloc::vec![],
            asked: true,
            pinned: false,
            used: 0,
        };
        std::fs::write(root.join(JOURNAL), render(core::slice::from_ref(&r))).unwrap();
        let (st, _) = Store::open(Dir::new(&root).unwrap()).unwrap();
        (root, st, r)
    }

    #[test]
    fn a_derived_component_is_the_signed_one_byte_for_byte() {
        let full = pack();
        let (root, mut st, old) = store("ok", &full);
        st.derive("zz", "places", Component::Substring, build)
            .unwrap();
        let r = st.packs().first().unwrap().clone();
        assert!(r.set.contains(Component::Substring));
        assert_eq!(std::fs::read(root.join(PACKS).join(&r.file)).unwrap(), full);
        assert!(
            !root.join(PACKS).join(&old.file).exists(),
            "the old file is retired"
        );
        // Twice: nothing to do.
        st.derive("zz", "places", Component::Substring, |_| {
            Err("not called".into())
        })
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_derived_section_that_differs_is_refused_and_nothing_changes() {
        let full = pack();
        let (root, mut st, old) = store("bad", &full);
        let err = st
            .derive("zz", "places", Component::Substring, |i| {
                let mut b = build(i)?;
                b[0] ^= 1;
                Ok(b)
            })
            .unwrap_err();
        assert!(matches!(err, StoreError::Derive(_)), "{err}");
        assert_eq!(st.packs(), core::slice::from_ref(&old));
        assert!(root.join(PACKS).join(&old.file).exists());
        // A component whose inputs are not installed is refused too.
        let err = st
            .derive("zz", "places", Component::AdminIndex, build)
            .unwrap_err();
        assert!(matches!(err, StoreError::Derive(_)), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }
}
