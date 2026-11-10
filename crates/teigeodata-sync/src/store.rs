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
//! All file system calls go through [`Fs`], so tests can crash the store at every step.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use std::io::{self, Read, Seek, Write};

use serde_json::{Map, Value};
use teigeodata_format::component::{Component, Components};

use crate::catalog::Entry;
use crate::fetch::{FetchError, Mirrors, Transport, install_from};
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
    /// The names of the files in a directory.
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
}

const JOURNAL: &str = "journal";
const JOURNAL_TMP: &str = "journal.tmp";
const PACKS: &str = "packs";
const STAGING: &str = "staging";
/// The journal's format version.
const VERSION: u64 = 1;

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
        Ok((Self { fs, packs }, rec))
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
    /// component. Staging is resumed if an earlier attempt left one. On success the new pack is
    /// in the journal and its file in `packs/`; the replaced one's file is removed, or left for
    /// the next recovery when it cannot be (in use).
    ///
    /// # Errors
    /// [`FetchError`]: the install's, or [`FetchError::Io`] for the file system. The journal
    /// is untouched by any failure before the commit.
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
        let staging = alloc::format!("{STAGING}/{file}.part");
        let mut f = self.fs.open(&staging)?;
        let done = install_from(transport, mirrors, path, entry, asset_len, set, &mut f)?;
        f.set_len(done.pack_len)?;
        f.sync()?;
        drop(f);
        let target = alloc::format!("{PACKS}/{file}");
        self.fs.rename(&staging, &target)?;
        self.fs.sync_dir(PACKS)?;
        self.fs.sync_dir(STAGING)?;
        let record = Record {
            address: entry.address.clone(),
            component: entry.component.clone(),
            release: entry.release.clone(),
            set,
            file,
            pack_len: done.pack_len,
            head: entry.head,
        };
        let mut packs = self.packs.clone();
        let old = packs
            .iter()
            .position(|r| r.address == record.address && r.component == record.component)
            .map(|i| packs.remove(i));
        packs.push(record);
        packs.sort_by(|a, b| (&a.address, &a.component).cmp(&(&b.address, &b.component)));
        self.commit(&packs)?;
        self.packs = packs;
        if let Some(old) = old.filter(|o| self.packs.iter().all(|r| r.file != o.file)) {
            // Best effort: a mapped file on Windows cannot go yet; recovery removes it later.
            if self.fs.remove(&Self::path_of(&old)).is_ok() {
                let _ = self.fs.sync_dir(PACKS);
            }
        }
        Ok(done)
    }

    /// Writes `packs` as the journal: temp file, sync, rename, sync the root.
    fn commit(&mut self, packs: &[Record]) -> io::Result<()> {
        let bytes = render(packs);
        let mut t = self.fs.open(JOURNAL_TMP)?;
        t.set_len(0)?;
        t.write_all(&bytes)?;
        t.sync()?;
        drop(t);
        self.fs.rename(JOURNAL_TMP, JOURNAL)?;
        self.fs.sync_dir("")
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
    let set = set.closed();
    let tag = if set == Components::all().closed() {
        "all".to_owned()
    } else {
        set.iter()
            .map(Component::name)
            .collect::<Vec<_>>()
            .join("+")
    };
    let name = alloc::format!("{base}.{tag}.tgd");
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
    if v.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err(invalid("the journal's version is unknown"));
    }
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
        for e in std::fs::read_dir(self.at(path))? {
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
