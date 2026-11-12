//! The bytes of a layer, and the views over them: the one module of the engine allowed to use
//! `unsafe` (D-113, B13). Everything else in the workspace keeps `forbid(unsafe_code)`; this
//! crate lowers it to `deny` so that the two blocks below can allow it, each with its reason.
//!
//! 1. **Mapping a file** (`memmap2::Mmap::map`). Rust cannot promise that a mapped file does
//!    not change under the mapping. The engine relies on the install rule of the [pack format]:
//!    a pack file is written once, under a new name, and never written over or truncated while
//!    it may be mapped. Removing it (unlink) is safe: the mapping keeps the old inode. A file the
//!    engine does not own is read into memory instead ([`Backing::read`]). If a file is
//!    truncated anyway, reading past its new end raises `SIGBUS` on POSIX; no safe library can
//!    catch that, so the rule is enforced where files are installed, and
//!    [`Layer::unchanged`](crate::Layer::unchanged) detects a file that was replaced or resized.
//! 2. **Views that borrow their own bytes** ([`Owned`]). An opened pack is a set of zero-copy
//!    views (`Pack<'a>`, `Places<'a>`, ...) borrowing the bytes the same struct owns. The views
//!    are built by [`Views::open`], which is generic over the lifetime and so cannot keep the
//!    bytes anywhere else; the bytes never move (a mapping, a boxed slice or a static slice);
//!    the views are dropped before the bytes (field order); and they are reached only through
//!    [`Owned::views`], which shortens their lifetime to the borrow of the owner. That last step
//!    compiles only because `Views` is covariant in its lifetime, which the compiler checks.
//!
//! [pack format]: https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/pack-format.md#physical-file-names-and-installs

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::Error;
use crate::layer::Views;

/// Where a layer's bytes live.
pub(crate) enum Backing {
    /// A read-only mapping of an installed pack file.
    #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
    Mapped(memmap2::Mmap),
    /// Bytes on the heap: a file read into memory, or bytes the caller handed over.
    Heap(Box<[u8]>),
    /// Bytes that live for the whole program, such as an embedded baseline.
    Static(&'static [u8]),
}

impl core::fmt::Debug for Backing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let kind = match self {
            #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
            Self::Mapped(_) => "mapped",
            Self::Heap(_) => "heap",
            Self::Static(_) => "static",
        };
        write!(f, "{kind} ({} bytes)", self.bytes().len())
    }
}

impl Backing {
    /// The bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
            Self::Mapped(m) => m,
            Self::Heap(b) => b,
            Self::Static(s) => s,
        }
    }

    /// Whether the bytes are a mapping.
    pub(crate) const fn is_mapped(&self) -> bool {
        match self {
            #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
            Self::Mapped(_) => true,
            _ => false,
        }
    }

    /// Maps `file` read-only. The file must follow the install rule (module docs).
    #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
    pub(crate) fn map(file: &File) -> io::Result<Self> {
        // An empty file cannot be mapped on every platform, and is no pack anyway.
        if file.metadata()?.len() == 0 {
            return Ok(Self::Heap(Box::default()));
        }
        #[allow(
            unsafe_code,
            reason = "B13/D-113: mapping is unsafe only if the file changes while mapped; pack \
                      files are immutable once installed (written once, renamed into place, \
                      never written over), and a file outside the store is read, not mapped"
        )]
        // SAFETY: see the reason above and the module documentation.
        let map = unsafe { memmap2::Mmap::map(file) }?;
        Ok(Self::Mapped(map))
    }

    /// Reads `path` into memory.
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Self::Heap(bytes.into_boxed_slice()))
    }
}

/// A layer's bytes and the views over them.
pub(crate) struct Owned {
    // Declared before `backing`, so dropped first: no view outlives the bytes.
    views: Views<'static>,
    backing: Backing,
}

impl core::fmt::Debug for Owned {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Owned")
            .field("backing", &self.backing)
            .finish_non_exhaustive()
    }
}

/// Shortens the views' lifetime to the borrow: compiles only if `Views` is covariant.
const fn covariant<'s>(v: &'s Views<'static>) -> &'s Views<'s> {
    v
}

impl Owned {
    /// Opens the views over `backing`'s bytes.
    ///
    /// # Errors
    /// As [`Views::open`].
    pub(crate) fn new(backing: Backing, reads: crate::layer::Reads) -> Result<Self, Error> {
        let bytes: &[u8] = backing.bytes();
        #[allow(
            unsafe_code,
            reason = "B13/D-113: the views borrow bytes owned by the same struct; the bytes \
                      never move (a mapping, a boxed slice or a static slice) and outlive the \
                      views (field order), `Views::open` is lifetime-generic so it keeps no \
                      other copy, and `Owned::views` shortens the lifetime to the owner's borrow"
        )]
        // SAFETY: see the reason above and the module documentation.
        let bytes: &'static [u8] = unsafe { &*core::ptr::from_ref::<[u8]>(bytes) };
        let views = Views::open(bytes, reads)?;
        Ok(Self { views, backing })
    }

    /// The views, borrowed from the owner.
    pub(crate) const fn views(&self) -> &Views<'_> {
        covariant(&self.views)
    }

    /// The bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        self.backing.bytes()
    }

    /// Whether the bytes are a mapping.
    pub(crate) const fn is_mapped(&self) -> bool {
        self.backing.is_mapped()
    }
}
