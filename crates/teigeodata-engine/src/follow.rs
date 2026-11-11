//! The snapshot swap on install (D-172): the engine follows the store's journal.
//!
//! [`Engine::follow`] opens each pack the journal lists that is not open yet, with the
//! component set it was installed with, drops each open layer from the store's `packs/`
//! directory that the journal no longer lists, and reopens one whose file was replaced (a
//! repair). [`Shared::follow`](crate::Shared::follow) does
//! it on the next engine and publishes once: queries already running keep the old layers
//! mapped until they end, and a pack file is never written over (D-113, D-171).

use std::path::PathBuf;

use teigeodata_sync::store::{Dir, Record};

use crate::{Engine, Error, Layer, LayerOptions, Shared};

/// What [`Engine::follow`] changed.
#[derive(Debug, Default)]
pub struct Followed {
    /// The files opened.
    pub opened: Vec<PathBuf>,
    /// The files dropped: no longer in the journal.
    pub closed: Vec<PathBuf>,
    /// The files that failed to open, with why: left out, so one damaged pack never hides the
    /// rest. Repair them and follow again.
    pub failed: Vec<(PathBuf, Error)>,
}

impl Engine {
    /// Makes the layers from `dir`'s `packs/` match `packs`, the store's journal: opens what is
    /// missing with `options` (the installed set from each record), drops what is gone, and
    /// reopens a layer whose file was replaced since (it is in both lists). Layers
    /// from anywhere else (the baseline, packs the app opened itself) are left alone. A pack
    /// whose open fails is reported in [`Followed::failed`]; with the same journal, a second
    /// call changes nothing more.
    pub fn follow(&mut self, dir: &Dir, packs: &[Record], options: LayerOptions) -> Followed {
        let root = dir.packs_dir();
        let wanted: Vec<PathBuf> = packs.iter().map(|r| dir.pack_path(r)).collect();
        let mut out = Followed::default();
        // Gone from the journal, or its file replaced since it was opened (a repair, D-172):
        // dropped here, and a listed one opened again below.
        let gone: Vec<_> = self
            .layers()
            .filter_map(|l| {
                let p = l.path()?;
                (p.parent() == Some(root.as_path())
                    && (!wanted.iter().any(|w| w == p) || !l.unchanged()))
                .then(|| (l.id(), p.to_path_buf()))
            })
            .collect();
        for (id, path) in gone {
            self.remove(id);
            out.closed.push(path);
        }
        for (r, path) in packs.iter().zip(wanted) {
            if self.layers().any(|l| l.path() == Some(path.as_path())) {
                continue;
            }
            let opts = LayerOptions {
                installed: Some(r.set),
                ..options
            };
            match Layer::open(&path, opts) {
                Ok(layer) => {
                    self.add(layer);
                    out.opened.push(path);
                }
                Err(e) => out.failed.push((path, e)),
            }
        }
        out
    }
}

impl Shared {
    /// [`Engine::follow`] on the next engine, published at once: running queries keep their
    /// snapshot, and the dropped packs stay mapped until the last one ends. Nothing is
    /// published when nothing changed.
    pub fn follow(&self, dir: &Dir, packs: &[Record], options: LayerOptions) -> Followed {
        let r: Result<Followed, Followed> = self.try_update(|e| {
            let f = e.follow(dir, packs, options);
            if f.opened.is_empty() && f.closed.is_empty() {
                Err(f)
            } else {
                Ok(f)
            }
        });
        r.unwrap_or_else(|f| f)
    }
}
