//! Core types of teigeodata, shared by the forge, the pack format and every query.
//!
//! - [`PlaceId`]: the stable 64-bit ID and its text form `tgd:<cc>/<serial>`
//!   ([ADR-0008](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0008-stable-ids.md), D-070).
//! - [`Point`]: a position on a fixed integer grid, 1e-6° ([`E6`]) for place points and 1e-7°
//!   ([`E7`]) for geometry
//!   ([ADR-0029](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0029-coordinate-grid.md)).
//! - [`hilbert`]: the Hilbert index that orders the rows of every pack (spike S1).
//!
//! `no_std`, no dependencies, no allocation, no panics.
//!
//! ```
//! use teigeodata_core::{E6, PlaceId, Point};
//!
//! let id: PlaceId = "tgd:np/3kq".parse()?;
//! assert_eq!(id.namespace().country(), Some(*b"np"));
//! assert_eq!(id.to_string(), "tgd:np/3kq");
//!
//! let kathmandu = Point::<E6>::from_degrees(27.7172, 85.324)?;
//! assert_eq!((kathmandu.lat(), kathmandu.lon()), (27_717_200, 85_324_000));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![no_std]

#[cfg(test)]
extern crate std;

mod grid;
mod hilbert;
mod id;

pub use grid::{E6, E7, Grid, GridError, Point};
pub use hilbert::hilbert;
pub use id::{IdError, Namespace, PlaceId, SERIAL_MAX};
