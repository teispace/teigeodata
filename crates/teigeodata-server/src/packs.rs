//! The pack set a server answers from (phase 6.1): pack files, the store's installed packs, or
//! both, behind a [`Shared`] engine (D-141). Each request loads a [`Snapshot`] without a lock;
//! [`Packs::follow`] makes the engine match the store's journal in one publish (D-172), so a
//! server keeps answering while `tgd install`, `update` or `remove` runs beside it.
//!
//! The server never opens the store for writing: it reads the journal alone
//! ([`Store::read_packs`]), so it never recovers, creates or removes a file an install in
//! progress is writing. Pack files are mapped (`Load::Map`, D-113): a pack the store replaces
//! or removes stays mapped until the last query that holds it ends.

use std::io;
use std::path::{Path, PathBuf};

use teigeodata_engine::{Engine, Error, Followed, Layer, LayerOptions, Load, Shared, Snapshot};
use teigeodata_sync::store::{Dir, Store};

/// Why a pack set could not be opened.
#[derive(Debug)]
pub enum PackError {
    /// A pack file could not be opened.
    Pack(PathBuf, Error),
    /// The store's journal could not be read.
    Store(PathBuf, io::Error),
}

impl core::fmt::Display for PackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Pack(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Store(p, e) => write!(f, "the store at {}: {e}", p.display()),
        }
    }
}

impl std::error::Error for PackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pack(_, e) => Some(e),
            Self::Store(_, e) => Some(e),
        }
    }
}

/// The store at `root`, which must exist: a server never creates one (`tgd init` does).
fn store_dir(root: &Path) -> Result<(PathBuf, Dir), PackError> {
    let fail = |e| PackError::Store(root.to_path_buf(), e);
    if !root.is_dir() {
        return Err(fail(io::Error::new(
            io::ErrorKind::NotFound,
            "no store here (set one up with `tgd init`)",
        )));
    }
    Ok((root.to_path_buf(), Dir::new(root).map_err(fail)?))
}

/// The packs a server answers from, shared by every request.
#[derive(Debug)]
pub struct Packs {
    shared: Shared,
    /// The store's root and directory.
    store: Option<(PathBuf, Dir)>,
}

/// How the server opens packs: mapped, verified as the engine does by default.
fn options() -> LayerOptions {
    LayerOptions {
        load: Load::Map,
        ..LayerOptions::default()
    }
}

impl Packs {
    /// The packs in `files`, in that order, and those installed in the store at `store` (each
    /// with its installed components and parts), if given. Opens nothing else; a server with no
    /// pack answers every query empty.
    ///
    /// # Errors
    /// A pack that does not open, or a store whose journal cannot be read. A store pack that
    /// fails to open is reported by [`Packs::follow`] instead, so one damaged pack never stops
    /// a server.
    pub fn open(files: &[PathBuf], store: Option<&Path>) -> Result<(Self, Followed), PackError> {
        let mut e = Engine::new();
        for p in files {
            e.add(Layer::open(p, options()).map_err(|x| PackError::Pack(p.clone(), x))?);
        }
        let packs = Self {
            shared: Shared::new(e),
            store: store.map(store_dir).transpose()?,
        };
        let followed = packs.follow()?;
        Ok((packs, followed))
    }

    /// The engine for one request: lock-free and allocation-free.
    #[must_use]
    pub fn load(&self) -> Snapshot {
        self.shared.load()
    }

    /// The shared engine, to update it another way (an app embedding the server).
    #[must_use]
    pub const fn shared(&self) -> &Shared {
        &self.shared
    }

    /// Makes the store's layers match its journal now: opens what was installed, drops what was
    /// removed, reopens what was repaired, and publishes once if anything changed. Packs from
    /// files are left alone. Without a store, changes nothing. Cheap when nothing changed (one
    /// read of the journal), so a server may call it on a timer.
    ///
    /// # Errors
    /// The journal could not be read or does not parse; the engine is left as it was.
    pub fn follow(&self) -> Result<Followed, PackError> {
        let Some((root, dir)) = &self.store else {
            return Ok(Followed::default());
        };
        let records =
            Store::read_packs(&mut dir.clone()).map_err(|e| PackError::Store(root.clone(), e))?;
        Ok(self.shared.follow(dir, &records, options()))
    }
}
