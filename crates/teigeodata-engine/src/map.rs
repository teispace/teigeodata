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

/// The bytes [`Views::open`] opens.
#[derive(Clone, Copy)]
pub(crate) enum Source<'a> {
    /// The whole file (holes where sections are not installed).
    Whole(&'a [u8]),
    /// A head and some sections ([`teigeodata_format::Pack::open_sections`]).
    Sections {
        head: &'a [u8],
        file_len: u64,
        sections: &'a [(u64, &'a [u8])],
    },
}

/// Where a layer's bytes live.
pub(crate) enum Backing {
    /// A read-only mapping of an installed pack file.
    #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
    Mapped(memmap2::Mmap),
    /// Bytes on the heap: a file read into memory, or bytes the caller handed over.
    Heap(Box<[u8]>),
    /// Bytes that live for the whole program, such as an embedded baseline.
    Static(&'static [u8]),
    /// A pack's head and some of its sections, each in its own buffer
    /// ([`Layer::from_sections`](crate::Layer::from_sections)).
    Sections {
        /// The file's first bytes, at least its head frame.
        head: Box<[u8]>,
        /// The file's length.
        file_len: u64,
        /// Each section held: its file offset and bytes, by offset.
        sections: Box<[(u64, Box<[u8]>)]>,
    },
}

impl core::fmt::Debug for Backing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let kind = match self {
            #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
            Self::Mapped(_) => "mapped",
            Self::Heap(_) => "heap",
            Self::Sections { .. } => "sections",
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
            Self::Sections { head, .. } => head,
        }
    }

    /// The pack file's length: the bytes held, or the whole file's for sections.
    pub(crate) fn file_len(&self) -> u64 {
        match self {
            Self::Sections { file_len, .. } => *file_len,
            b => b.bytes().len() as u64,
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

    /// Maps `length` bytes at `offset` of `file` read-only: a pack stored whole inside another
    /// file, such as an uncompressed asset of an Android APK. The offset need not be page
    /// aligned (the mapping starts at the page below and the bytes are the range). The file
    /// must follow the install rule (module docs); an installed APK does.
    #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
    pub(crate) fn map_range(file: &File, offset: u64, length: u64) -> io::Result<Self> {
        check_range(file, offset, length)?;
        if length == 0 {
            return Ok(Self::Heap(Box::default()));
        }
        let len = usize::try_from(length)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "range too long to map"))?;
        #[allow(
            unsafe_code,
            reason = "B13/D-113: as `map`, for a range of a file that is never written while \
                      mapped (an installed pack or app package)"
        )]
        // SAFETY: see the reason above and the module documentation.
        let map = unsafe {
            memmap2::MmapOptions::new()
                .offset(offset)
                .len(len)
                .map(file)
        }?;
        Ok(Self::Mapped(map))
    }

    /// Reads `length` bytes at `offset` of `path` into memory.
    pub(crate) fn read_range(path: &Path, offset: u64, length: u64) -> io::Result<Self> {
        use std::io::Seek as _;
        let mut file = File::open(path)?;
        check_range(&file, offset, length)?;
        file.seek(io::SeekFrom::Start(offset))?;
        let len = usize::try_from(length)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "range too long to read"))?;
        let mut bytes = vec![0; len];
        file.read_exact(&mut bytes)?;
        Ok(Self::Heap(bytes.into_boxed_slice()))
    }

    /// Reads `path` into memory.
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Self::Heap(bytes.into_boxed_slice()))
    }
}

/// Refuses a range that does not lie inside `file`.
fn check_range(file: &File, offset: u64, length: u64) -> io::Result<()> {
    let size = file.metadata()?.len();
    match offset.checked_add(length) {
        Some(end) if end <= size => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("bytes {offset}..+{length} are not inside a file of {size} bytes"),
        )),
    }
}

/// A layer's bytes and the views over them.
pub(crate) struct Owned {
    // Declared before `index` and `backing`, so dropped first: no view outlives the bytes.
    views: Views<'static>,
    /// For [`Backing::Sections`]: each section's offset and bytes, borrowed from `backing`.
    #[allow(
        dead_code,
        reason = "held for the views, which borrow it; never read directly"
    )]
    index: Box<[(u64, &'static [u8])]>,
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

/// `r` with its lifetime extended: only for data on the heap, in a mapping or static, owned by
/// an [`Owned`] that outlives every borrow (see [`Owned::new`]).
fn stable<T: ?Sized>(r: &T) -> &'static T {
    #[allow(
        unsafe_code,
        reason = "B13/D-113: the views borrow bytes owned by the same struct; the bytes \
                  never move (a mapping, a boxed slice or a static slice, never the struct's \
                  own fields) and outlive the views (field order), `Views::open` is \
                  lifetime-generic so it keeps no other copy, and `Owned::views` shortens the \
                  lifetime to the owner's borrow"
    )]
    // SAFETY: see the reason above and the module documentation.
    unsafe {
        &*core::ptr::from_ref::<T>(r)
    }
}

impl Owned {
    /// Opens the views over `backing`'s bytes.
    ///
    /// # Errors
    /// As [`Views::open`].
    pub(crate) fn new(backing: Backing, reads: crate::layer::Reads) -> Result<Self, Error> {
        let (views, index) = if let Backing::Sections {
            head,
            file_len,
            sections,
        } = &backing
        {
            // Each borrow is of a boxed buffer, which stays put when `backing` moves.
            let index: Box<[(u64, &'static [u8])]> =
                sections.iter().map(|(o, b)| (*o, stable(&**b))).collect();
            let src = Source::Sections {
                head: stable(&**head),
                file_len: *file_len,
                sections: stable(&*index),
            };
            (Views::open(src, reads)?, index)
        } else {
            let src = Source::Whole(stable(backing.bytes()));
            (Views::open(src, reads)?, Box::default())
        };
        Ok(Self {
            views,
            index,
            backing,
        })
    }

    /// The views, borrowed from the owner.
    pub(crate) const fn views(&self) -> &Views<'_> {
        covariant(&self.views)
    }

    /// The pack file's length.
    pub(crate) fn file_len(&self) -> u64 {
        self.backing.file_len()
    }

    /// Whether the bytes are a mapping.
    pub(crate) const fn is_mapped(&self) -> bool {
        self.backing.is_mapped()
    }
}
