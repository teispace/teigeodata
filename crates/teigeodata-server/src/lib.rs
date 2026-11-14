//! The request core of the teigeodata servers (phase 6, [D-310]): what the HTTP server, the MCP
//! server and `tgd` share, with no runtime of its own.
//!
//! - [`packs`]: the pack set, files or the store, behind a shared engine that follows the
//!   store's journal (D-141, D-172).
//! - [`query`]: what a client may ask, its bounds, the stable error codes, and the one place each
//!   query runs.
//! - [`output`]: the one mapping from a place to each output (ADR-0038) and the attribution of
//!   the packs that answered (D-032).
//! - [`seams`]: who is asking, may they, how much they used, open by default (ADR-0025).
//! - `http` (feature `http`): the HTTP server over axum (D-309).
//! - `mcp` (feature `mcp`): the MCP server over rmcp (D-315).
//! - `catalog` (feature `store`): the store's catalog, its rows and `coverage` (D-318).
//!
//! [D-310]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/log.md

#[cfg(any(feature = "http", feature = "fuzz", feature = "mcp"))]
mod body;
#[cfg(feature = "store")]
pub mod catalog;
#[cfg(any(feature = "http", feature = "fuzz"))]
mod compat;
/// The fuzz harness of the query parsing.
#[cfg(feature = "fuzz")]
pub mod fuzz;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod output;
pub mod packs;
#[cfg(any(feature = "http", feature = "fuzz"))]
mod params;
pub mod query;
pub mod seams;
