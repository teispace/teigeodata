//! A layer: one open pack, its views, its coverage, its precedence and the state of its lazy
//! verification (D-078, D-113).

use core::cmp::Ordering;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering as Atomic};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use teigeodata_core::{E7, PlaceId, Point};
use teigeodata_format::arcs::Arcs;
use teigeodata_format::blocks::{BLOCK, Blocks};
use teigeodata_format::hierarchy::Hierarchy;
use teigeodata_format::idmap::IdMap;
use teigeodata_format::names::Names;
use teigeodata_format::place_index::PlaceIndex;
use teigeodata_format::places::Places;
use teigeodata_format::polygons::Polygons;
use teigeodata_format::redirects::Redirects;
use teigeodata_format::region::Region;
use teigeodata_format::search::Search;
use teigeodata_format::{Bbox, Kind, Pack};

use crate::Error;
use crate::map::{Backing, Owned};

/// When a layer's section hashes are checked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verify {
    /// On first use (D-078): a section under 1 MiB is hashed whole the first time a query
    /// needs it; a larger one has its block hashes proved when the layer opens and each 16 KiB
    /// block checked once, remembered in a bitmap. Opening costs only the head checks and the
    /// proofs.
    #[default]
    Lazy,
    /// Every section at open (`Pack::open_verified`).
    Eager,
    /// Never: for bytes already verified by their installer (a deep `verify` after download).
    Trusted,
}

/// How a pack file becomes bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Load {
    /// Map the file (native, feature `mmap`); read it where mapping is not available. Only for
    /// files that follow the install rule: never written over while mapped (D-113).
    #[default]
    Map,
    /// Read the whole file into memory: for files the engine does not own.
    Read,
}

/// Options for opening a layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerOptions {
    /// The developer's priority: a higher one wins over every other rule (default 0).
    pub priority: i32,
    /// The embedded baseline: always the lowest layer, whatever its priority.
    pub baseline: bool,
    /// When sections are verified.
    pub verify: Verify,
    /// How a file is loaded.
    pub load: Load,
}

/// A layer's handle in an [`Engine`](crate::Engine): stable while the layer is in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub(crate) u32);

/// Section states for lazy verification.
const UNCHECKED: u8 = 0;
const GOOD: u8 = 1;
const BAD: u8 = 2;
/// Kinds below this have a state slot; the engine reads no kind above 31.
const KINDS: usize = 32;

/// The zero-copy views of one pack, opened once (structure checks only).
#[derive(Debug)]
pub(crate) struct Views<'a> {
    pub(crate) pack: Pack<'a>,
    pub(crate) ids: Option<IdMap<'a>>,
    pub(crate) places: Option<Places<'a>>,
    pub(crate) hierarchy: Option<Hierarchy<'a>>,
    pub(crate) redirects: Option<Redirects<'a>>,
    pub(crate) names: Option<Names<'a>>,
    pub(crate) search: Option<Search<'a>>,
    pub(crate) place_index: Option<PlaceIndex<'a>>,
    pub(crate) zones: Option<Region<'a>>,
    pub(crate) admin: Option<Region<'a>>,
    pub(crate) polygons: Option<Polygons<'a>>,
    /// The block hashes of every section of 1 MiB or more, proved against its section hash.
    pub(crate) blocks: Vec<(Kind, Blocks<'a, 'static>)>,
}

impl<'a> Views<'a> {
    /// Opens `bytes` as a pack and every section the engine reads: the head checks, the
    /// structure of each view and the proof of every block-hash list. No section is hashed.
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self, Error> {
        let pack = Pack::open(bytes)?;
        let get = |k| pack.section(k);
        let ids = get(Kind::IDMAP).map(|s| IdMap::open(&s)).transpose()?;
        let places = get(Kind::PLACES).map(|s| Places::open(&s)).transpose()?;
        let hierarchy = get(Kind::HIERARCHY)
            .map(|s| Hierarchy::open(&s))
            .transpose()?;
        let redirects = get(Kind::REDIRECTS)
            .map(|s| Redirects::open(&s))
            .transpose()?;
        let names = get(Kind::NAMES).map(|s| Names::open(&s)).transpose()?;
        let search = get(Kind::SEARCH).map(|s| Search::open(&s)).transpose()?;
        let place_index = match (get(Kind::PLACE_INDEX), places) {
            (Some(s), Some(p)) => Some(PlaceIndex::open(&s, p)?),
            _ => None,
        };
        let zones = get(Kind::REGION).map(|s| Region::open(&s)).transpose()?;
        let arcs = get(Kind::ARCS).map(|s| Arcs::open(&s)).transpose()?;
        let admin = match (get(Kind::ADMIN_REGION), &arcs) {
            (Some(s), Some(a)) => Some(Region::open_on_arcs(&s, a)?),
            (Some(s), None) => Some(Region::open(&s)?),
            (None, _) => None,
        };
        let polygons = get(Kind::POLYGONS)
            .map(|s| Polygons::open(&s))
            .transpose()?;
        let mut blocks = Vec::new();
        for s in pack.sections() {
            if let Some(b) = pack.blocks(s.kind())? {
                blocks.push((s.kind(), b));
            }
        }
        Ok(Self {
            pack,
            ids,
            places,
            hierarchy,
            redirects,
            names,
            search,
            place_index,
            zones,
            admin,
            polygons,
            blocks,
        })
    }

    /// The PLACES row of region `r` of an admin or zone index: its own link, else POLYGONS.
    pub(crate) fn region_row(&self, region: &Region<'_>, r: u32, admin: bool) -> Option<u32> {
        region.place(r).or_else(|| {
            admin
                .then_some(self.polygons.as_ref())
                .flatten()
                .and_then(|p| p.row(r as usize))
        })
    }
}

/// What identifies a file on disk, to notice it was replaced or resized while mapped.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    fn of(path: &Path, file: &File) -> std::io::Result<Self> {
        let m = file.metadata()?;
        Ok(Self {
            path: path.to_owned(),
            len: m.len(),
            modified: m.modified().ok(),
        })
    }
}

/// One open pack in an [`Engine`](crate::Engine).
#[derive(Debug)]
pub struct Layer {
    pub(crate) owned: Owned,
    options: LayerOptions,
    bbox: Option<Bbox>,
    /// The smallest and largest ID the pack knows, current or retired.
    id_range: Option<(u64, u64)>,
    state: [AtomicU8; KINDS],
    /// One bit per verified block, per entry of `Views::blocks`.
    memo: Vec<Box<[AtomicUsize]>>,
    stamp: Option<FileStamp>,
    pub(crate) id: LayerId,
}

impl Layer {
    /// Opens the pack file at `path`, mapped by default (D-113).
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be read, [`Error::Format`] if it is not a valid pack.
    pub fn open(path: impl AsRef<Path>, options: LayerOptions) -> Result<Self, Error> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let stamp = FileStamp::of(path, &file)?;
        let backing = match options.load {
            #[cfg(all(feature = "mmap", not(target_arch = "wasm32")))]
            Load::Map => Backing::map(&file)?,
            #[cfg(not(all(feature = "mmap", not(target_arch = "wasm32"))))]
            Load::Map => Backing::read(path)?,
            Load::Read => Backing::read(path)?,
        };
        drop(file);
        Self::with(backing, options, Some(stamp))
    }

    /// Opens a pack held in memory.
    ///
    /// # Errors
    /// [`Error::Format`] if it is not a valid pack.
    pub fn from_bytes(bytes: impl Into<Box<[u8]>>, options: LayerOptions) -> Result<Self, Error> {
        Self::with(Backing::Heap(bytes.into()), options, None)
    }

    /// Opens a pack that lives for the whole program (an embedded baseline).
    ///
    /// # Errors
    /// [`Error::Format`] if it is not a valid pack.
    pub fn from_static(bytes: &'static [u8], options: LayerOptions) -> Result<Self, Error> {
        Self::with(Backing::Static(bytes), options, None)
    }

    fn with(
        backing: Backing,
        options: LayerOptions,
        stamp: Option<FileStamp>,
    ) -> Result<Self, Error> {
        let owned = Owned::new(backing)?;
        let v = owned.views();
        if options.verify == Verify::Eager {
            v.pack.verify_sections()?;
        }
        let ranges = [
            v.ids.as_ref().and_then(IdMap::id_range),
            v.redirects.as_ref().and_then(Redirects::id_range),
        ];
        let id_range = ranges.into_iter().flatten().fold(None, |acc, (lo, hi)| {
            let (lo, hi) = (lo.get(), hi.get());
            Some(match acc {
                None => (lo, hi),
                Some((a, b)) => (lo.min(a), hi.max(b)),
            })
        });
        let memo = v
            .blocks
            .iter()
            .map(|(_, b)| (0..b.memo_words()).map(|_| AtomicUsize::new(0)).collect())
            .collect();
        let initial = if options.verify == Verify::Lazy {
            UNCHECKED
        } else {
            GOOD
        };
        let bbox = v.pack.manifest().bbox();
        Ok(Self {
            options,
            bbox,
            id_range,
            state: core::array::from_fn(|_| AtomicU8::new(initial)),
            memo,
            stamp,
            owned,
            id: LayerId(0),
        })
    }

    /// The pack's address (`np`, `ae-du`).
    #[must_use]
    pub fn address(&self) -> &str {
        self.owned.views().pack.manifest().address()
    }

    /// The pack's component (`places`).
    #[must_use]
    pub fn component(&self) -> &str {
        self.owned.views().pack.manifest().component()
    }

    /// The pack's release (`2026.11.0`).
    #[must_use]
    pub fn release(&self) -> &str {
        self.owned.views().pack.manifest().release()
    }

    /// The options it was opened with.
    #[must_use]
    pub const fn options(&self) -> &LayerOptions {
        &self.options
    }

    /// The coverage box from the manifest, if the pack has one.
    #[must_use]
    pub const fn bbox(&self) -> Option<Bbox> {
        self.bbox
    }

    /// Its handle in the engine that holds it.
    #[must_use]
    pub const fn id(&self) -> LayerId {
        self.id
    }

    /// Whether the bytes are a file mapping.
    #[must_use]
    pub const fn is_mapped(&self) -> bool {
        self.owned.is_mapped()
    }

    /// The pack's length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.owned.bytes().len()
    }

    /// Whether the pack is empty (never, for an open pack).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.owned.bytes().is_empty()
    }

    /// Whether the file it was opened from still has the length and modification time it had
    /// then (always true for bytes in memory). A mapped file that changed breaks the install
    /// rule (D-113): drop the layer and open the new file.
    #[must_use]
    pub fn unchanged(&self) -> bool {
        self.stamp.as_ref().is_none_or(|s| {
            File::open(&s.path)
                .and_then(|f| FileStamp::of(&s.path, &f))
                .is_ok_and(|now| now == *s)
        })
    }

    pub(crate) fn views(&self) -> &Views<'_> {
        self.owned.views()
    }

    /// Whether the pack may hold `id`, current or retired: O(1).
    pub(crate) fn may_hold(&self, id: PlaceId) -> bool {
        self.id_range
            .is_some_and(|(lo, hi)| (lo..=hi).contains(&id.get()))
    }

    /// Whether the coverage box holds `at` (true without a box).
    pub(crate) fn covers(&self, at: Point<E7>) -> bool {
        self.bbox.is_none_or(|b| {
            let (lat, lon) = (at.lat(), at.lon());
            let lat_in = (b.min.lat()..=b.max.lat()).contains(&lat);
            let lon_in = if b.min.lon() <= b.max.lon() {
                (b.min.lon()..=b.max.lon()).contains(&lon)
            } else {
                lon >= b.min.lon() || lon <= b.max.lon()
            };
            lat_in && lon_in
        })
    }

    /// Checks section `kind` before a query reads it, once (D-078): whole under 1 MiB, every
    /// block of a covered section otherwise. Free after the first time: one atomic load.
    ///
    /// # Errors
    /// [`Error::Integrity`] if its bytes do not match its hash, now or earlier.
    pub(crate) fn ensure(&self, kind: Kind) -> Result<(), Error> {
        let Some(slot) = usize::try_from(kind.0).ok().and_then(|k| self.state.get(k)) else {
            return Ok(());
        };
        match slot.load(Atomic::Acquire) {
            GOOD => return Ok(()),
            BAD => return Err(self.integrity(kind)),
            _ => {}
        }
        let v = self.views();
        let ok = match v.blocks.iter().position(|(k, _)| *k == kind) {
            Some(i) => {
                let len = v.blocks.get(i).map_or(0, |(_, b)| b.len());
                self.verify_blocks(i, 0..len).is_ok()
            }
            None => v.pack.section(kind).is_none_or(|s| s.verify().is_ok()),
        };
        slot.store(if ok { GOOD } else { BAD }, Atomic::Release);
        if ok {
            Ok(())
        } else {
            Err(self.integrity(kind))
        }
    }

    /// The bytes `range` of section `kind`, each 16 KiB block they touch checked once (D-078):
    /// for readers that know which bytes they need. A section under 1 MiB is checked whole.
    ///
    /// # Errors
    /// [`Error::Integrity`] if a block (or the section) differs from its hash;
    /// [`Error::Range`] if the section is missing or the range is outside it.
    pub fn section_bytes(
        &self,
        kind: Kind,
        range: core::ops::Range<usize>,
    ) -> Result<&[u8], Error> {
        let v = self.views();
        let section = v.pack.section(kind).ok_or(Error::Range)?;
        let out = section.bytes().get(range.clone()).ok_or(Error::Range)?;
        match v.blocks.iter().position(|(k, _)| *k == kind) {
            Some(i) if !range.is_empty() => {
                self.verify_blocks(i, range.start / BLOCK..(range.end - 1) / BLOCK + 1)?;
            }
            Some(_) => {}
            None => self.ensure(kind)?,
        }
        Ok(out)
    }

    /// Checks blocks `blocks` of covered section `i`, skipping those already checked.
    fn verify_blocks(&self, i: usize, blocks: core::ops::Range<usize>) -> Result<(), Error> {
        let v = self.views();
        let (Some((kind, b)), Some(memo)) = (v.blocks.get(i), self.memo.get(i)) else {
            return Ok(());
        };
        let bits = usize::BITS as usize;
        for block in blocks {
            let word = memo.get(block / bits);
            let mask = 1 << (block % bits);
            if word.is_some_and(|w| w.load(Atomic::Acquire) & mask != 0) {
                continue;
            }
            b.verify(block).map_err(|_| self.integrity(*kind))?;
            if let Some(w) = word {
                w.fetch_or(mask, Atomic::Release);
            }
        }
        Ok(())
    }

    fn integrity(&self, kind: Kind) -> Error {
        Error::Integrity {
            address: self.address().into(),
            kind,
        }
    }

    /// The precedence order of §3.4 of the engine note: `Less` means `self` wins.
    pub(crate) fn precedence(&self, other: &Self) -> Ordering {
        let (a, b) = (&self.options, &other.options);
        let digest = |l: &Self| *l.views().pack.head_digest();
        a.baseline
            .cmp(&b.baseline)
            .then_with(|| b.priority.cmp(&a.priority))
            .then_with(|| release_cmp(other.release(), self.release()))
            .then_with(|| specificity(other.address()).cmp(&specificity(self.address())))
            .then_with(|| self.address().cmp(other.address()))
            .then_with(|| self.component().cmp(other.component()))
            .then_with(|| digest(self).cmp(&digest(other)))
    }
}

/// How specific an address is: its number of `-`-separated parts (`ae-du` is 2, `ae` 1).
fn specificity(address: &str) -> usize {
    address.split('-').count()
}

/// Orders releases as dotted numbers (`2026.9.0 < 2026.11.0`): parts split at `.` and `-`,
/// numeric parts compared as numbers and before text parts, text parts as text.
#[must_use]
pub fn release_cmp(left: &str, right: &str) -> Ordering {
    let (mut pa, mut pb) = (left.split(['.', '-']), right.split(['.', '-']));
    loop {
        let (x, y) = match (pa.next(), pb.next()) {
            (Some(x), Some(y)) => (x, y),
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        };
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(nx), Ok(ny)) => nx.cmp(&ny).then_with(|| x.len().cmp(&y.len())),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{release_cmp, specificity};
    use core::cmp::Ordering::{Equal, Greater, Less};

    #[test]
    fn releases_compare_as_dotted_numbers() {
        assert_eq!(release_cmp("2026.9.0", "2026.11.0"), Less);
        assert_eq!(release_cmp("2026.11.0", "2026.11.0"), Equal);
        assert_eq!(release_cmp("2026.12.0", "2026.11.5"), Greater);
        assert_eq!(release_cmp("2026.11", "2026.11.0"), Less);
        assert_eq!(release_cmp("2026.11.0", "2026.11.0-rc1"), Less);
        // Text would say otherwise.
        assert!("2026.9.0" > "2026.11.0");
        // Leading zeros are a different, later spelling, so the order stays total.
        assert_eq!(release_cmp("2026.011.0", "2026.11.0"), Greater);
    }

    #[test]
    fn longer_addresses_are_more_specific() {
        assert!(specificity("ae-du") > specificity("ae"));
        assert_eq!(specificity("in-br-t07"), 3);
    }
}
