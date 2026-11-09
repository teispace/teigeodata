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

use core::fmt;

pub use layer::{Layer, LayerId, LayerOptions, Load, Verify, release_cmp};
pub use query::{Hit, Near, Place, Scratch};
pub use teigeodata_format::Kind;
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
#[derive(Debug, Default)]
pub struct Engine {
    layers: Vec<Layer>,
    next: u32,
}

impl Engine {
    /// An engine with no layers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        self.layers.insert(at, layer);
        id
    }

    /// Removes a layer and returns it; dropping it unmaps its bytes.
    pub fn remove(&mut self, id: LayerId) -> Option<Layer> {
        let at = self.layers.iter().position(|l| l.id == id)?;
        Some(self.layers.remove(at))
    }

    /// The layers, highest precedence first.
    pub fn layers(&self) -> impl ExactSizeIterator<Item = &Layer> {
        self.layers.iter()
    }

    /// The layer with this handle.
    #[must_use]
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
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
