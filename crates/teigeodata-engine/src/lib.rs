//! The layered engine of teigeodata ([ADR-0011], [engine]).
//!
//! An [`Engine`] holds open packs as **layers** and answers every query across them as if they
//! were one dataset:
//!
//! - [`Layer::open`] maps a pack file (D-113; `unsafe` lives only in the `map` module) and checks
//!   its head. Sections are verified lazily (D-078): a section under 1 MiB is hashed whole the
//!   first time a query reads it, a larger one 16 KiB block at a time ([`Verify`]).
//! - Layers are ordered by **precedence** (the engine note, §3.4): the developer's priority,
//!   then the newer release, then the more specific address, then the address text; the
//!   baseline is always last. The winning layer for an ID decides it: its row is the answer,
//!   its redirect sends a lookup to the survivor, its tombstone hides the ID in every layer
//!   below.
//! - Queries route to the layers that can answer ([`Engine::get`] by ID range, point queries by
//!   coverage box), merge per-layer answers (k-way, by distance or score) and tag each answer
//!   with the layer it came from ([`Engine::tier`]). Hot paths write into caller-owned
//!   [`Scratch`] and output buffers, so a warm query does not allocate (M1).
//!
//! The answers do not depend on how places are split into packs or in which order the packs
//! were added: the metamorphic gate (`tests/metamorphic.rs`) splits packs and compares.
//!
//! ```
//! use teigeodata_engine::{Engine, Scratch};
//! use teigeodata_core::{E6, Point};
//!
//! let engine = Engine::new();
//! let mut scratch = Scratch::default();
//! let mut out = Vec::new();
//! let at = Point::<E6>::from_degrees(27.7, 85.3)?;
//! engine.reverse(at, 5, &mut scratch, &mut out)?;
//! assert!(out.is_empty()); // no layers yet
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [ADR-0011]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0011-layered-engine.md
//! [engine]: https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/engine.md

mod layer;
mod map;
mod query;
#[cfg(feature = "search")]
mod search;
#[cfg(feature = "shared")]
mod shared;

use core::fmt;
use std::sync::Arc;

pub use layer::{Layer, LayerId, LayerOptions, Load, Verify, release_cmp};
pub use query::{Hit, Near, Place, Scratch};

/// The radius in metres for [`Engine::countries_near`] over the embedded baseline: its
/// outlines are simplified at the 2 km level (main, D-157; lite at 500 m, D-147), whose
/// nested Douglas–Peucker importances let a border stray up to 2.5 km from the source
/// (measured, the locator gate), so at this radius no country whose border is near is
/// missed (D-149).
#[cfg(feature = "admin-index")]
pub const LOCATOR_RADIUS_M: u32 = 3_000;
#[cfg(feature = "shared")]
pub use shared::{Shared, Snapshot};
pub use teigeodata_format::Kind;
pub use teigeodata_format::component::{Component, Components};
pub use teigeodata_format::place_index::Rect;

/// Why the engine could not open a pack or answer a query.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A file could not be read.
    Io(std::io::Error),
    /// The bytes are not a valid pack, or a section's structure is broken.
    Format(teigeodata_format::Error),
    /// A section's bytes do not match its hash: the layer is corrupt or was changed.
    Integrity {
        /// The layer's address.
        address: Box<str>,
        /// The section.
        kind: Kind,
    },
    /// A section is missing or a byte range is outside it.
    Range,
    /// The query needs a component the layer has but was not installed with (D-133): install
    /// it, or ask only what the installed components answer.
    MissingComponent {
        /// The layer's address.
        address: Box<str>,
        /// The component.
        component: Component,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading a pack: {e}"),
            Self::Format(e) => write!(f, "not a valid pack: {e}"),
            Self::Integrity { address, kind } => {
                write!(f, "{address}: section {kind} does not match its hash")
            }
            Self::Range => f.write_str("no such section or byte range"),
            Self::MissingComponent { address, component } => write!(
                f,
                "{address}: the `{}` component is not installed",
                component.name()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Format(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<teigeodata_format::Error> for Error {
    fn from(e: teigeodata_format::Error) -> Self {
        Self::Format(e)
    }
}

/// Where an answer came from (the engine note, §2): the baseline or an installed pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier<'e> {
    /// The embedded baseline.
    Baseline,
    /// An installed pack.
    Pack {
        /// Its address (`np`).
        address: &'e str,
        /// Its component (`places`).
        component: &'e str,
        /// Its release (`2026.11.0`).
        release: &'e str,
    },
}

/// A set of layers answering as one dataset. Layers are kept in precedence order.
///
/// An engine is a snapshot: cloning it shares its layers (each an [`Arc`]), so a new set of
/// packs is built beside the one queries are using and published at once
/// (`Shared`). A layer's file stays mapped until the last snapshot holding it drops.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    layers: Vec<Arc<Layer>>,
    next: u32,
}

/// The embedded baseline (feature `baseline`, step 4.2): every country, its first-level units,
/// the cities of 15,000 people or more and their timezones, one pack built by `forge embed`
/// (`baseline-main.tgd`, the main build, or `baseline-lite.tgd`, the lite build). The build reads it from the
/// absolute path in `TEIGEODATA_BASELINE`, so an application picks the build it ships. Targets
/// that would rather not embed it (WASM, D-144 Z3) load the same file with
/// [`Layer::from_bytes`] and [`LayerOptions::baseline`].
#[cfg(feature = "baseline")]
pub static BASELINE: &[u8] = include_bytes!(env!(
    "TEIGEODATA_BASELINE",
    "the `baseline` feature embeds the baseline pack: set TEIGEODATA_BASELINE to the absolute path of a baseline-main.tgd or baseline-lite.tgd (built by `forge embed`)"
));

impl Engine {
    /// An engine with no layers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An engine holding the embedded [`BASELINE`], borrowed for the program's life: the answer
    /// wherever no installed pack has one.
    ///
    /// # Errors
    /// [`Error::Format`] if the embedded file is not a valid pack.
    #[cfg(feature = "baseline")]
    pub fn with_baseline() -> Result<Self, Error> {
        let mut e = Self::new();
        e.add(Layer::from_static(BASELINE, LayerOptions::baseline())?);
        Ok(e)
    }

    /// Adds a layer at its place in the precedence order and returns its handle. The order
    /// depends only on the layers, never on the order they were added.
    pub fn add(&mut self, mut layer: Layer) -> LayerId {
        layer.id = LayerId(self.next);
        self.next = self.next.wrapping_add(1);
        let id = layer.id;
        let at = self
            .layers
            .partition_point(|l| l.precedence(&layer).is_le());
        self.layers.insert(at, Arc::new(layer));
        id
    }

    /// Removes a layer and returns it; its bytes are unmapped when the last snapshot holding
    /// it drops.
    pub fn remove(&mut self, id: LayerId) -> Option<Arc<Layer>> {
        let at = self.layers.iter().position(|l| l.id == id)?;
        Some(self.layers.remove(at))
    }

    /// The layers, highest precedence first.
    pub fn layers(&self) -> impl ExactSizeIterator<Item = &Layer> {
        self.layers.iter().map(|l| &**l)
    }

    /// The layer with this handle.
    #[must_use]
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id).map(|l| &**l)
    }

    /// The tier of an answer from layer `id`.
    #[must_use]
    pub fn tier(&self, id: LayerId) -> Option<Tier<'_>> {
        self.layer(id).map(|l| {
            if l.options().baseline {
                Tier::Baseline
            } else {
                Tier::Pack {
                    address: l.address(),
                    component: l.component(),
                    release: l.release(),
                }
            }
        })
    }
}

#[cfg(test)]
mod thread_safety {
    /// An engine and its layers are shared across threads (a snapshot per query).
    #[test]
    fn engines_and_layers_are_send_and_sync() {
        fn shared<T: Send + Sync>() {}
        shared::<super::Engine>();
        shared::<super::Layer>();
    }
}
