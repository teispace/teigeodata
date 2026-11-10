//! One error type for the sync crate. `Copy`, no allocation: every message is static, with the
//! index it is about.

use core::fmt;

/// Why a transport asset, a plan or a write was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The seek table is malformed, or disagrees with the asset or the pack it describes.
    SeekTable(&'static str),
    /// A frame is malformed: its sizes are wrong, or it does not decode to its stated bytes.
    Frame {
        /// The frame's index in the seek table.
        index: usize,
        /// What is wrong.
        problem: &'static str,
    },
    /// The pack layout cannot be cut into transport frames.
    Layout(&'static str),
    /// A wanted byte range runs past the pack or ends before it starts.
    Range,
    /// The pack itself was refused.
    Pack(teigeodata_format::Error),
    /// The writer was given something it cannot write, or its output failed the self-check.
    Write(&'static str),
    /// An install was refused: the asset disagrees with its signed catalog entry, or the caller
    /// fed it out of order.
    Install(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeekTable(p) => write!(f, "malformed seek table: {p}"),
            Self::Frame { index, problem } => write!(f, "malformed frame {index}: {problem}"),
            Self::Layout(p) => write!(f, "the pack cannot be framed: {p}"),
            Self::Range => f.write_str("a wanted byte range is outside the pack"),
            Self::Pack(e) => write!(f, "the pack was refused: {e}"),
            Self::Write(p) => write!(f, "cannot write the transport asset: {p}"),
            Self::Install(p) => write!(f, "the install was refused: {p}"),
        }
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Pack(e) => Some(e),
            _ => None,
        }
    }
}

impl From<teigeodata_format::Error> for Error {
    fn from(e: teigeodata_format::Error) -> Self {
        Self::Pack(e)
    }
}
