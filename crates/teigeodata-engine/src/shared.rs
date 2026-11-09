//! The current set of packs, shared by every thread (D-141). Queries run on a [`Snapshot`],
//! an immutable [`Engine`] loaded without a lock; installing or removing packs builds the next
//! engine beside it (sharing the open layers) and publishes it in one atomic store. A query
//! never sees half an update, never waits for a writer, and keeps the packs it started with
//! mapped until it ends.
//!
//! Chosen by the L5 A/B ([benchmarks](../../../docs/02-research/benches/phase-4.md)): with
//! 8 reader threads and a writer, `arc-swap` costs about 1.4× a bare query against 4.6× for
//! a held `RwLock` (whose writer waited up to 26 ms) and 14× for a `Mutex`.

use std::sync::{Arc, Mutex, PoisonError};

use arc_swap::{ArcSwap, Guard};

use crate::Engine;

/// The current engine of a process, published atomically.
#[derive(Debug)]
pub struct Shared {
    current: ArcSwap<Engine>,
    /// Serialises writers, so two updates never build on the same snapshot.
    write: Mutex<()>,
}

/// The engine a query runs on: the current one when it was loaded. Meant to live for one
/// query or a few; hold [`Shared::current`] for longer.
#[derive(Debug)]
pub struct Snapshot(Guard<Arc<Engine>>);

impl core::ops::Deref for Snapshot {
    type Target = Engine;

    fn deref(&self) -> &Engine {
        &self.0
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new(Engine::new())
    }
}

impl Shared {
    /// Shares `engine` as the current one.
    #[must_use]
    pub fn new(engine: Engine) -> Self {
        Self {
            current: ArcSwap::from_pointee(engine),
            write: Mutex::new(()),
        }
    }

    /// The current engine, for one query: lock-free and allocation-free.
    #[must_use]
    pub fn load(&self) -> Snapshot {
        Snapshot(self.current.load())
    }

    /// The current engine as an owned handle, to keep across many queries.
    #[must_use]
    pub fn current(&self) -> Arc<Engine> {
        self.current.load_full()
    }

    /// Builds the next engine from the current one with `change` (adding and removing
    /// layers) and publishes it whole. Queries already running keep their snapshot.
    pub fn update<R>(&self, change: impl FnOnce(&mut Engine) -> R) -> R {
        let _writer = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = Engine::clone(&self.current.load());
        let out = change(&mut next);
        self.current.store(Arc::new(next));
        out
    }

    /// As [`update`](Self::update), publishing only when `change` succeeds: on an error the
    /// current engine stays as it was.
    ///
    /// # Errors
    /// The error of `change`.
    pub fn try_update<R, E>(
        &self,
        change: impl FnOnce(&mut Engine) -> Result<R, E>,
    ) -> Result<R, E> {
        let _writer = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = Engine::clone(&self.current.load());
        let out = change(&mut next)?;
        self.current.store(Arc::new(next));
        Ok(out)
    }
}
