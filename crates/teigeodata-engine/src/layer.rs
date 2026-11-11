//! A layer: one open pack, its views, its coverage, its precedence and the state of its lazy
//! verification (D-078, D-113).

use core::cmp::Ordering;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering as Atomic};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use teigeodata_core::PlaceId;
#[cfg(any(feature = "timezone-index", feature = "admin-index", feature = "dem"))]
use teigeodata_core::{E7, Point};
#[cfg(feature = "admin-index")]
use teigeodata_format::arcs::ArcChunks;
#[cfg(feature = "boundaries")]
use teigeodata_format::arcs::Arcs;
use teigeodata_format::blocks::{BLOCK, Blocks};
use teigeodata_format::component::{Component, Components};
#[cfg(feature = "dem")]
use teigeodata_format::dem::Dem;
#[cfg(feature = "elevation")]
use teigeodata_format::elevation::Elevations;
#[cfg(feature = "admin")]
use teigeodata_format::hierarchy::Hierarchy;
use teigeodata_format::idmap::IdMap;
#[cfg(feature = "admin")]
use teigeodata_format::iso_codes::IsoCodes;
#[cfg(feature = "names")]
use teigeodata_format::names::Names;
use teigeodata_format::place_index::PlaceIndex;
#[cfg(feature = "timezone")]
use teigeodata_format::place_tz::PlaceTz;
use teigeodata_format::places::Places;
#[cfg(feature = "boundaries")]
use teigeodata_format::poly_index::PolyIndex;
#[cfg(feature = "boundaries")]
use teigeodata_format::polygons::Polygons;
use teigeodata_format::redirects::Redirects;
#[cfg(any(feature = "timezone-index", feature = "admin-index"))]
use teigeodata_format::region::Region;
#[cfg(feature = "search")]
use teigeodata_format::search::Search;
#[cfg(feature = "search")]
use teigeodata_format::search::rules::Rule;
#[cfg(feature = "tz-rules")]
use teigeodata_format::tz_rules::TzRules;
use teigeodata_format::{Bbox, Kind, Pack, release_cmp};
#[cfg(feature = "search")]
use teigeodata_names::PhoneticKey;

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
    /// The components installed (D-133): `None` for a whole pack. A section of a component not
    /// in the set is never read or hashed (its bytes may be a hole), and a query that needs it
    /// returns [`Error::MissingComponent`]. The set must hold what its components need, and
    /// `point` whenever the pack has it.
    pub installed: Option<Components>,
}

impl LayerOptions {
    /// The options of a baseline layer: last in precedence, whatever its priority.
    #[must_use]
    pub fn baseline() -> Self {
        Self {
            baseline: true,
            ..Self::default()
        }
    }
}

/// A layer's handle in an [`Engine`](crate::Engine): stable while the layer is in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub(crate) u32);

/// Section states for lazy verification.
const UNCHECKED: u8 = 0;
const GOOD: u8 = 1;
const BAD: u8 = 2;
/// The section's component is not installed.
const MISSING: u8 = 3;
/// Kinds below this have a state slot; the engine reads no kind above 31.
const KINDS: usize = 32;

/// The components whose Cargo feature is off: this build has no reader for them, so their
/// sections are never read and their queries do not exist (D-137). `point` is always built;
/// `postal` and `extra` are read raw through [`Layer::section_bytes`].
const NOT_BUILT: &[Component] = &[
    #[cfg(not(feature = "elevation"))]
    Component::Elevation,
    #[cfg(not(feature = "admin"))]
    Component::Admin,
    #[cfg(not(feature = "names"))]
    Component::Names,
    #[cfg(not(feature = "search"))]
    Component::Search,
    #[cfg(not(feature = "timezone"))]
    Component::Timezone,
    #[cfg(not(feature = "timezone-index"))]
    Component::TimezoneIndex,
    #[cfg(not(feature = "boundaries"))]
    Component::Boundaries,
    #[cfg(not(feature = "admin-index"))]
    Component::AdminIndex,
    #[cfg(not(feature = "dem"))]
    Component::Dem,
    #[cfg(not(feature = "tz-rules"))]
    Component::TzRules,
];

/// Where the block-hash list of every installed section is (located, not proved: D-142).
fn located_blocks<'a>(
    pack: &Pack<'a>,
    missing: Components,
) -> Result<Vec<(Kind, Blocks<'a, 'static>)>, Error> {
    let mut blocks = Vec::new();
    for s in pack.sections() {
        if Component::of(s.kind()).is_some_and(|c| !missing.contains(c))
            && let Some(b) = pack.locate_blocks(s.kind())?
        {
            blocks.push((s.kind(), b));
        }
    }
    Ok(blocks)
}

/// The zero-copy views of one pack, opened once (structure checks only).
#[derive(Debug)]
pub(crate) struct Views<'a> {
    pub(crate) pack: Pack<'a>,
    pub(crate) ids: Option<IdMap<'a>>,
    pub(crate) places: Option<Places<'a>>,
    /// Heights, one per PLACES row (D-133); absent in a pack without them.
    #[cfg(feature = "elevation")]
    pub(crate) heights: Option<Elevations<'a>>,
    /// Each place's timezone area (D-134); absent in a pack without them.
    #[cfg(feature = "timezone")]
    pub(crate) place_tz: Option<PlaceTz<'a>>,
    #[cfg(feature = "admin")]
    pub(crate) hierarchy: Option<Hierarchy<'a>>,
    /// The ISO 3166 codes of the coded rows (D-197); absent in a pack without them.
    #[cfg(feature = "admin")]
    pub(crate) iso: Option<IsoCodes<'a>>,
    pub(crate) redirects: Option<Redirects<'a>>,
    #[cfg(feature = "names")]
    pub(crate) names: Option<Names<'a>>,
    #[cfg(feature = "search")]
    pub(crate) search: Option<Search<'a>>,
    /// SEARCH's query rules, read once here so a search decodes none of them (D-120).
    #[cfg(feature = "search")]
    pub(crate) rules: QueryRules<'a>,
    pub(crate) place_index: Option<PlaceIndex<'a>>,
    #[cfg(feature = "timezone-index")]
    pub(crate) zones: Option<Region<'a>>,
    #[cfg(feature = "admin-index")]
    pub(crate) admin: Option<Region<'a>>,
    #[cfg(feature = "boundaries")]
    pub(crate) polygons: Option<Polygons<'a>>,
    /// The outlines and their box index: `countries_near` (D-149), `bbox_of`, `boundary`.
    #[cfg(feature = "boundaries")]
    pub(crate) arcs: Option<Arcs<'a>>,
    #[cfg(feature = "boundaries")]
    pub(crate) poly_index: Option<PolyIndex<'a>>,
    /// Terrain heights at any point (D-192); absent in a pack without them.
    #[cfg(feature = "dem")]
    pub(crate) dem: Option<Dem<'a>>,
    /// The rules of the pack's timezones (D-196); absent in a pack without them.
    #[cfg(feature = "tz-rules")]
    pub(crate) tz_rules: Option<TzRules<'a>>,
    /// The block hashes of every installed section of 1 MiB or more, located but not proved:
    /// each list is proved against its section hash on the section's first use (D-142).
    pub(crate) blocks: Vec<(Kind, Blocks<'a, 'static>)>,
    /// The components the pack has sections of but that were not installed.
    pub(crate) missing: Components,
}

impl<'a> Views<'a> {
    /// Opens `bytes` as a pack and every installed section the engine reads: the head checks,
    /// the structure of each view and where each block-hash list is. No section is hashed, no
    /// block-hash list is proved (D-142), and no section of a component outside `installed` is
    /// read.
    #[allow(clippy::too_many_lines, reason = "one view per section kind, in order")]
    pub(crate) fn open(bytes: &'a [u8], installed: Option<Components>) -> Result<Self, Error> {
        // A partial install may have holes, BLOCK_HASHES among them: only the head is read.
        let pack = match installed {
            None => Pack::open(bytes)?,
            Some(_) => Pack::open_head(bytes)?,
        };
        let missing = missing(&pack, installed)?;
        let get = |k| {
            pack.section(k)
                .filter(|_| Component::of(k).is_none_or(|c| !missing.contains(c)))
        };
        let ids = get(Kind::IDMAP).map(|s| IdMap::open(&s)).transpose()?;
        let places = get(Kind::PLACES).map(|s| Places::open(&s)).transpose()?;
        #[cfg(feature = "elevation")]
        let heights = get(Kind::ELEVATION)
            .map(|s| Elevations::open(&s))
            .transpose()?;
        #[cfg(feature = "timezone")]
        let place_tz = get(Kind::PLACE_TZ).map(|s| PlaceTz::open(&s)).transpose()?;
        // Per-place sections are one row per PLACES row, or the layer is refused.
        #[cfg(any(feature = "elevation", feature = "timezone"))]
        let rows = places.map(|p| p.len());
        #[cfg(feature = "elevation")]
        one_per_place(Kind::ELEVATION, heights.map(|h| h.len()), rows)?;
        #[cfg(feature = "timezone")]
        one_per_place(Kind::PLACE_TZ, place_tz.map(|t| t.len()), rows)?;
        #[cfg(feature = "admin")]
        let hierarchy = get(Kind::HIERARCHY)
            .map(|s| Hierarchy::open(&s))
            .transpose()?;
        #[cfg(feature = "admin")]
        let iso = get(Kind::ISO_CODES)
            .zip(places)
            .map(|(s, p)| IsoCodes::open(&s, p.len()));
        let redirects = get(Kind::REDIRECTS)
            .map(|s| Redirects::open(&s))
            .transpose()?;
        #[cfg(feature = "names")]
        let names = get(Kind::NAMES).map(|s| Names::open(&s)).transpose()?;
        #[cfg(feature = "search")]
        let search = get(Kind::SEARCH).map(|s| Search::open(&s)).transpose()?;
        #[cfg(feature = "search")]
        let rules = search.as_ref().map(QueryRules::new).unwrap_or_default();
        let place_index = match (get(Kind::PLACE_INDEX), places) {
            (Some(s), Some(p)) => Some(PlaceIndex::open(&s, p)?),
            _ => None,
        };
        #[cfg(feature = "timezone-index")]
        let zones = get(Kind::REGION).map(|s| Region::open(&s)).transpose()?;
        #[cfg(feature = "boundaries")]
        let arcs = get(Kind::ARCS).map(|s| Arcs::open(&s)).transpose()?;
        #[cfg(feature = "admin-index")]
        let admin = match (get(Kind::ADMIN_REGION), &arcs) {
            (Some(s), Some(a)) => Some(Region::open_on_arcs(&s, a)?),
            (Some(s), None) => Some(Region::open(&s)?),
            (None, _) => None,
        };
        #[cfg(feature = "boundaries")]
        let polygons = get(Kind::POLYGONS)
            .map(|s| Polygons::open(&s))
            .transpose()?;
        #[cfg(feature = "boundaries")]
        let poly_index = match (get(Kind::POLY_INDEX), &polygons) {
            (Some(s), Some(p)) => Some(PolyIndex::open(&s, p)?),
            _ => None,
        };
        #[cfg(feature = "dem")]
        let dem = get(Kind::DEM).map(|s| Dem::open(&s)).transpose()?;
        #[cfg(feature = "tz-rules")]
        let tz_rules = get(Kind::TZ_RULES).map(|s| TzRules::open(&s)).transpose()?;
        let blocks = located_blocks(&pack, missing)?;
        Ok(Self {
            pack,
            ids,
            places,
            #[cfg(feature = "elevation")]
            heights,
            #[cfg(feature = "timezone")]
            place_tz,
            #[cfg(feature = "admin")]
            hierarchy,
            // Rows past PLACES are refused when it opens; without PLACES it has no rows.
            #[cfg(feature = "admin")]
            iso: iso.transpose()?,
            redirects,
            #[cfg(feature = "names")]
            names,
            #[cfg(feature = "search")]
            search,
            #[cfg(feature = "search")]
            rules,
            place_index,
            #[cfg(feature = "timezone-index")]
            zones,
            #[cfg(feature = "admin-index")]
            admin,
            #[cfg(feature = "boundaries")]
            polygons,
            #[cfg(feature = "boundaries")]
            arcs,
            #[cfg(feature = "boundaries")]
            poly_index,
            #[cfg(feature = "dem")]
            dem,
            #[cfg(feature = "tz-rules")]
            tz_rules,
            blocks,
            missing,
        })
    }

    /// Whether section `kind` is installed: its component is, or it is `BLOCK_HASHES` and an
    /// installed section has blocks.
    pub(crate) fn installed(&self, kind: Kind) -> bool {
        match Component::of(kind) {
            Some(c) => !self.missing.contains(c),
            None if kind == Kind::BLOCK_HASHES => !self.blocks.is_empty(),
            None => true,
        }
    }

    /// The PLACES row of region `r` of the admin index: its own link, else POLYGONS'.
    #[cfg(feature = "admin-index")]
    pub(crate) fn admin_row(&self, region: &Region<'_>, r: u32) -> Option<u32> {
        region
            .place(r)
            .or_else(|| self.polygons.as_ref().and_then(|p| p.row(r as usize)))
    }
}

/// The components `pack` has sections of that are not in `installed` (none for `None`).
/// Refuses a set that lacks what its components need, or lacks `point` when the pack has it:
/// every other component answers through the places.
fn missing(pack: &Pack<'_>, installed: Option<Components>) -> Result<Components, Error> {
    let off = Components::of(NOT_BUILT);
    let has = present(pack);
    let Some(set) = installed else {
        // A whole pack: what this build has no reader for is missing.
        return Ok(has.without(Components::all().without(off)));
    };
    // A need counts only where the pack has it: a pack without `tz-rules` (built before
    // D-196, or the baseline) still installs `timezone` alone.
    let lacking = set
        .closed()
        .without(set)
        .iter()
        .find(|&c| has.contains(c))
        .or_else(|| {
            (has.contains(Component::Point) && !set.contains(Component::Point))
                .then_some(Component::Point)
        });
    if let Some(component) = lacking {
        return Err(Error::MissingComponent {
            address: pack.manifest().address().into(),
            component,
        });
    }
    // Installed but not built: as good as missing.
    Ok(has.without(set.without(off)))
}

/// Refuses a per-place section (`len` rows) that is not one row per PLACES row (`rows`).
#[cfg(any(feature = "elevation", feature = "timezone"))]
fn one_per_place(kind: Kind, len: Option<usize>, rows: Option<usize>) -> Result<(), Error> {
    if len.is_some() && len != rows {
        return Err(Error::Format(teigeodata_format::Error::Payload {
            kind: kind.0,
            column: 0,
            problem: "not one row per PLACES row",
        }));
    }
    Ok(())
}

/// The components `pack` has sections of.
fn present(pack: &Pack<'_>) -> Components {
    let list: Vec<Component> = pack
        .sections()
        .filter_map(|s| Component::of(s.kind()))
        .collect();
    Components::of(&list)
}

/// A layer's query rules (D-120) as slices of its SEARCH section, by kind.
#[cfg(feature = "search")]
#[derive(Debug, Default)]
pub(crate) struct QueryRules<'a> {
    /// The phonetic key the PHONETIC map was written with, if the reader knows it.
    pub(crate) phonetic: Option<PhoneticKey>,
    /// Unit-type abbreviations (folded) and their types.
    pub(crate) units: Vec<(&'a str, u8)>,
    /// Droppable suffixes, folded.
    pub(crate) drops: Vec<&'a str>,
    /// Native generics and one Latin spelling each, the longest native first; the spellings
    /// of one native are adjacent.
    pub(crate) natives: Vec<(&'a str, &'a str)>,
}

#[cfg(feature = "search")]
impl<'a> QueryRules<'a> {
    pub(crate) fn new(search: &Search<'a>) -> Self {
        let mut r = Self::default();
        for rule in &search.rules() {
            match rule {
                Rule::Phonetic(n) => r.phonetic = r.phonetic.or(PhoneticKey::from_name(n)),
                Rule::Unit { abbreviation, unit } => r.units.push((abbreviation, unit)),
                Rule::Drop(d) => r.drops.push(d),
                Rule::Native { native, spelling } => r.natives.push((native, spelling)),
            }
        }
        // Stable: equal natives stay adjacent, in stored order.
        r.natives.sort_by_key(|n| core::cmp::Reverse(n.0.len()));
        r
    }

    /// The type of unit abbreviation `a`, if a rule names it.
    pub(crate) fn unit(&self, a: &str) -> Option<u8> {
        self.units.iter().find(|u| u.0 == a).map(|u| u.1)
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
    /// Whether each entry of `Views::blocks` is proved against its section hash: `UNCHECKED`,
    /// `GOOD` or `BAD` (D-142).
    proved: Box<[AtomicU8]>,
    stamp: Option<FileStamp>,
    /// The arcs' chunk boxes for `countries_near`, built on its first call (D-149).
    #[cfg(feature = "admin-index")]
    chunks: std::sync::OnceLock<ArcChunks>,
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
        let owned = Owned::new(backing, options.installed)?;
        let v = owned.views();
        if options.verify == Verify::Eager {
            for s in v.pack.sections().filter(|s| v.installed(s.kind())) {
                s.verify()?;
            }
            for (_, b) in &v.blocks {
                b.prove()?;
            }
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
        let proof = if options.verify == Verify::Eager {
            GOOD
        } else {
            UNCHECKED
        };
        let proved = v.blocks.iter().map(|_| AtomicU8::new(proof)).collect();
        let bbox = v.pack.manifest().bbox();
        Ok(Self {
            options,
            bbox,
            id_range,
            state: core::array::from_fn(|k| {
                let kind = Kind(u32::try_from(k).unwrap_or(u32::MAX));
                AtomicU8::new(if v.installed(kind) { initial } else { MISSING })
            }),
            memo,
            proved,
            stamp,
            #[cfg(feature = "admin-index")]
            chunks: std::sync::OnceLock::new(),
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

    /// The notices of the sources the pack was built from, one per line (D-126): show them
    /// wherever the data is shown. `None` for a pack built before they were carried.
    #[must_use]
    pub fn attribution(&self) -> Option<&str> {
        self.owned.views().pack.manifest().attribution()
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

    /// The file it was opened from; `None` for bytes in memory.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.stamp.as_ref().map(|s| s.path.as_path())
    }

    /// Whether the bytes are a file mapping.
    #[must_use]
    pub const fn is_mapped(&self) -> bool {
        self.owned.is_mapped()
    }

    /// The number of places (PLACES rows) in the pack; 0 without PLACES.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.owned.views().places.map_or(0, |p| p.len())
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

    /// The chunk boxes of the layer's arcs, built from ARCS on the first call (O(vertices),
    /// about 40 B per 128 vertices); `None` without arcs. The caller ensures ARCS first.
    #[cfg(feature = "admin-index")]
    pub(crate) fn chunks(&self) -> Option<&ArcChunks> {
        let arcs = self.views().arcs.as_ref()?;
        Some(self.chunks.get_or_init(|| ArcChunks::build(arcs)))
    }

    /// Whether the pack may hold `id`, current or retired: O(1).
    pub(crate) fn may_hold(&self, id: PlaceId) -> bool {
        self.id_range
            .is_some_and(|(lo, hi)| (lo..=hi).contains(&id.get()))
    }

    /// Whether the coverage box holds `at` (true without a box).
    #[cfg(any(feature = "timezone-index", feature = "admin-index", feature = "dem"))]
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
            MISSING => return Err(self.missing(kind)),
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
        if !v.installed(kind) {
            return Err(self.missing(kind));
        }
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
        let (Some((kind, b)), Some(memo), Some(proof)) =
            (v.blocks.get(i), self.memo.get(i), self.proved.get(i))
        else {
            return Ok(());
        };
        // The list is proved once, on the first read of its section (D-142); a list that does
        // not prove fails every read of that section and nothing else.
        match proof.load(Atomic::Acquire) {
            GOOD => {}
            BAD => return Err(self.integrity(*kind)),
            _ => {
                let ok = b.prove().is_ok();
                proof.store(if ok { GOOD } else { BAD }, Atomic::Release);
                if !ok {
                    return Err(self.integrity(*kind));
                }
            }
        }
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

    fn missing(&self, kind: Kind) -> Error {
        Error::MissingComponent {
            address: self.address().into(),
            component: Component::of(kind).unwrap_or(Component::Point),
        }
    }

    /// The components installed and present in the pack.
    #[must_use]
    pub fn components(&self) -> Components {
        let v = self.views();
        present(&v.pack).without(v.missing)
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
