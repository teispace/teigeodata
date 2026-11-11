//! One error type for reading and writing. `Copy`, no allocation: every message is static, with
//! the index or tag it is about.

use core::fmt;

/// Why a pack was refused, or could not be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Shorter than a header and a footer.
    Truncated,
    /// The first or last eight bytes are not the pack magic.
    Magic,
    /// The pack needs a newer reader.
    Version {
        /// The reader major version the pack needs.
        min_reader_major: u16,
    },
    /// The head (header, table, manifest) does not match its hash: corrupted.
    HeadHash,
    /// The footer, the header and the real file length disagree.
    Length,
    /// The header is malformed.
    Header(&'static str),
    /// A section-table entry is malformed.
    Section {
        /// The entry's index in the table.
        index: u32,
        /// What is wrong.
        problem: &'static str,
    },
    /// The manifest is malformed.
    Manifest {
        /// The byte offset in the manifest.
        offset: u32,
        /// What is wrong.
        problem: &'static str,
    },
    /// A section's bytes do not match their BLAKE3 hash.
    SectionHash {
        /// The section's kind.
        kind: u32,
    },
    /// A section's payload is malformed (its frame, a column, or the data across columns).
    Payload {
        /// The section's kind.
        kind: u32,
        /// The column it is about; 0 for the section as a whole.
        column: u16,
        /// What is wrong.
        problem: &'static str,
    },
    /// The signature is missing, from an unknown key, or invalid.
    Signature(&'static str),
    /// The writer was given something it cannot write.
    Write(&'static str),
    /// A query's input, or bytes handed to a build-side parser (TZif, a TZ string), is
    /// invalid or out of range.
    Input(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("not a pack: shorter than a header and a footer"),
            Self::Magic => f.write_str("not a pack: wrong magic bytes"),
            Self::Version { min_reader_major } => write!(
                f,
                "the pack needs reader version {min_reader_major}; this reader is {}",
                crate::READER_MAJOR
            ),
            Self::HeadHash => f.write_str("the pack head is corrupted: its hash does not match"),
            Self::Length => f.write_str("the pack is truncated or has extra bytes"),
            Self::Header(p) => write!(f, "malformed pack header: {p}"),
            Self::Section { index, problem } => write!(f, "malformed section {index}: {problem}"),
            Self::Manifest { offset, problem } => {
                write!(f, "malformed manifest at byte {offset}: {problem}")
            }
            Self::SectionHash { kind } => write!(
                f,
                "section {} is corrupted: its hash does not match",
                crate::Kind(*kind)
            ),
            Self::Payload {
                kind,
                column: 0,
                problem,
            } => write!(f, "section {}: {problem}", crate::Kind(*kind)),
            Self::Payload {
                kind,
                column,
                problem,
            } => write!(
                f,
                "section {}, column {column}: {problem}",
                crate::Kind(*kind)
            ),
            Self::Signature(p) => write!(f, "pack signature: {p}"),
            Self::Write(p) => write!(f, "cannot write the pack: {p}"),
            Self::Input(p) => write!(f, "invalid input: {p}"),
        }
    }
}

impl core::error::Error for Error {}
