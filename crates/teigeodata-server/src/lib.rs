//! The request core of the teigeodata servers (phase 6, [D-309]): what the HTTP server, the MCP
//! server and `tgd` share, with no runtime of its own.
//!
//! - [`output`]: the one mapping from a place to each output (ADR-0038) and the attribution of
//!   the packs that answered (D-032).
//!
//! [D-309]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/log.md

pub mod output;
