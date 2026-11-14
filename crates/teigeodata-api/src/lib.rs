//! The safe public API of teigeodata.
//!
//! This crate is the single source of every language binding ([ADR-0016]). `cargo xtask
//! bindings` reads the `pub fn`s below with `syn`, writes `bindings/idl.json` and generates the
//! C ABI, its header and the Swift, Dart, Python, Node and JVM layers from it. A function
//! added here appears in every language after one `cargo xtask bindings`.
//!
//! **Boundary types.** A public function may use only these, so that every language can
//! express it:
//! - parameters: `bool`, `i32`, `u32`, `i64`, `u64`, `f64`, `&str`, `&[u8]`, a handle by
//!   reference (`&Engine`), and one callback `&mut dyn FnMut(u32) -> bool` (return `false` to
//!   cancel);
//! - results: `()`, the scalars, `String`, [`Blob`] and a handle, optionally inside [`Result`].
//!
//! **Handles** (D-274) are public structs with private fields; their `impl`'s `pub fn`s take
//! `&self` or no receiver, so each handle is `Send + Sync` and callable from any thread.
//!
//! **Records** (D-276) are public structs with only public fields, each a scalar, an `Option`
//! of one, an enum, or a `String`, deriving `Default` so that every field's default is its
//! zero value (empty text);
//! they are passed by reference (`&SearchOptions`). **Enums** (D-277) have unit variants
//! valued 0, 1, 2 … in order, `#[default]` on the one valued 0.
//!
//! The extractor rejects anything else with the line number.
//!
//! [ADR-0016]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0016-bindings.md
//!
//! ```
//! assert_eq!(teigeodata_api::fold("São Paulo"), "sao paulo");
//! ```

mod places;
#[cfg(feature = "store")]
mod store;

use std::cell::RefCell;
#[cfg(feature = "store")]
use std::sync::Arc;

pub use teigeodata_blob::Blob;
use teigeodata_blob::Column;
use teigeodata_core::{E6, E7, PlaceId, Point};
use teigeodata_engine::{Hit, Layer, LayerOptions, Near, Place, PostalCode, Scratch, Shared};
use teigeodata_format::Part;
use teigeodata_format::component::{Component, Components, MAX_LANGUAGES, Parts};

use places::Places;

/// Error codes, stable across every language. Zero is success at the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
    /// An argument is outside its documented range.
    InvalidArgument = 1,
    /// A callback asked to stop.
    Cancelled = 2,
    /// A bug inside teigeodata (a panic caught at the boundary, or a broken invariant).
    Internal = 3,
    /// A file could not be read.
    Io = 4,
    /// The bytes are not a valid pack, or a section does not match its hash.
    Corrupt = 5,
    /// The query needs a component of a pack that is not installed: install it, or ask a
    /// pack that has it.
    MissingComponent = 6,
    /// The network failed: no mirror answered, or the connection broke. Try again later.
    Network = 7,
    /// The store refused: the catalog did not verify, a pack is not in it or not installed,
    /// a pack another needs, or the store's files cannot be written.
    Refused = 8,
}

/// A failed call: a stable code and a message for people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The stable code.
    pub code: ErrorCode,
    /// What went wrong, in English.
    pub message: String,
}

impl Error {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<teigeodata_engine::Error> for Error {
    fn from(e: teigeodata_engine::Error) -> Self {
        use teigeodata_engine::Error as E;
        let code = match &e {
            E::Io(_) => ErrorCode::Io,
            E::Format(_) | E::Integrity { .. } | E::Range => ErrorCode::Corrupt,
            // A missing component reads as absent where a query allows it; elsewhere the
            // caller learns which one to install.
            E::MissingComponent { .. } => ErrorCode::MissingComponent,
            _ => ErrorCode::InvalidArgument,
        };
        Self::new(code, e.to_string())
    }
}

/// The result of a fallible call.
pub type Result<T> = core::result::Result<T, Error>;

/// Most rows [`sample_rows`] builds in one call.
pub const SAMPLE_ROWS_MAX: u32 = 1_000_000;

/// Most places one query returns.
pub const LIMIT_MAX: u32 = 10_000;

/// How [`Engine::search_with`] ranks (D-226). Every field's zero value is its default
/// (D-276).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchOptions {
    /// Prefer places near this latitude (with `focus_lon`): the user's location, or a point
    /// in their home country with a wide scale.
    pub focus_lat: Option<f64>,
    /// The focus longitude; set with `focus_lat` or not at all.
    pub focus_lon: Option<f64>,
    /// The distance in metres at which the focus gain halves (a place at the focus gains 30,
    /// one scale away 15); `None` is 50 km.
    pub focus_scale_m: Option<f64>,
    /// Rank by match tier first (exact, loose, prefix, one edit, substring, two edits,
    /// phonetic), then importance and focus, instead of one additive score.
    pub strict: bool,
}

impl SearchOptions {
    /// The engine's options, checked.
    fn engine(&self) -> Result<teigeodata_engine::SearchOptions> {
        let mut out = teigeodata_engine::SearchOptions::default().with_strict(self.strict);
        match (self.focus_lat, self.focus_lon) {
            (None, None) if self.focus_scale_m.is_none() => {}
            (Some(lat), Some(lon)) => {
                let mut focus = teigeodata_engine::Focus::new(point::<E6>(lat, lon)?);
                if let Some(scale) = self.focus_scale_m {
                    if !(scale.is_finite() && scale > 0.0) {
                        return Err(Error::new(
                            ErrorCode::InvalidArgument,
                            format!("focus scale is {scale} m; it must be positive"),
                        ));
                    }
                    focus.scale_m = scale;
                }
                out = out.with_focus(focus);
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a focus needs both focus_lat and focus_lon",
                ));
            }
        }
        Ok(out)
    }
}

/// When a pack's sections are verified against their hashes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verify {
    /// On first use: each section, or each 16 KiB block of a large one, is checked the first
    /// time a query reads it. Opening costs only the head checks.
    #[default]
    Lazy = 0,
    /// Every section when the pack opens.
    Eager = 1,
    /// Never: only for bytes already verified, by the store's installer for instance.
    Trusted = 2,
}

/// How a pack file is loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Load {
    /// Map the file into memory where the platform can, read it otherwise. The file must not
    /// be written over while it is open.
    #[default]
    Map = 0,
    /// Read the whole file into memory: for files the application does not own.
    Read = 1,
}

/// How [`Engine::open_file_with`] and [`Engine::open_bytes_with`] open a pack. Every field's
/// zero value is its default (D-276).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    /// Precedence over the other packs: a higher one wins where packs disagree (default 0).
    pub priority: i32,
    /// When sections are verified.
    pub verify: Verify,
    /// How a file is loaded; ignored for bytes.
    pub load: Load,
    /// The deepest admin level whose outlines are read, in a pack that splits its outlines by
    /// level: admin units below it are not found at a point, and their outlines are not
    /// loaded. `None` reads every level.
    pub outline_level: Option<u32>,
    /// The largest simplification of the outlines read, in metres, when the pack has coarser
    /// outlines; 0 is full resolution. Admin units at a point may then differ within that
    /// distance of a border.
    pub outline_tolerance_m: u32,
    /// The components the pack holds, by name and comma-separated (`point,names,search`), for
    /// a pack installed in part; what they need is added. Empty for a whole pack. A query
    /// that needs a component left out answers nothing, or `MissingComponent` where it cannot.
    pub components: String,
    /// The languages whose names and search sections are read, as comma-separated BCP 47
    /// tags (`en,ne`), at most 8; a language without a part of its own reads the pack's
    /// shared part. Empty reads every language.
    pub languages: String,
}

impl OpenOptions {
    /// The engine's options, checked.
    fn layer(&self) -> Result<LayerOptions> {
        let outline_level = match self.outline_level {
            None => None,
            Some(level) => Some(u8::try_from(level).map_err(|_| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("outline level {level} is above 255"),
                )
            })?),
        };
        let installed = if self.components.trim().is_empty() {
            None
        } else {
            let mut set = Components::NONE;
            for name in self.components.split(',').map(str::trim) {
                let c = Component::from_name(name).ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidArgument,
                        format!("`{name}` is not a component"),
                    )
                })?;
                set = set.union(Components::of(&[c]));
            }
            Some(set.closed())
        };
        let mut installed_parts = Parts::ALL;
        if !self.languages.trim().is_empty() {
            let mut keys = Vec::new();
            for tag in self.languages.split(',').map(str::trim) {
                keys.push(Part::language(tag).ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidArgument,
                        format!("`{tag}` is not a language tag"),
                    )
                })?);
            }
            installed_parts = Parts::ALL.languages(&keys).ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{} languages; at most {MAX_LANGUAGES}", keys.len()),
                )
            })?;
        }
        Ok(LayerOptions {
            priority: self.priority,
            verify: match self.verify {
                Verify::Lazy => teigeodata_engine::Verify::Lazy,
                Verify::Eager => teigeodata_engine::Verify::Eager,
                Verify::Trusted => teigeodata_engine::Verify::Trusted,
            },
            load: match self.load {
                Load::Map => teigeodata_engine::Load::Map,
                Load::Read => teigeodata_engine::Load::Read,
            },
            outline_level,
            outline_tolerance_m: self.outline_tolerance_m,
            installed,
            installed_parts,
            ..LayerOptions::default()
        })
    }
}

/// Open packs answering place queries (D-141, D-274). Every method takes `&self`: one engine
/// answers from any number of threads at once, and adding a pack publishes a new snapshot
/// while queries go on.
///
/// Place results are blobs with the columns `id`, `name`, `kind`, `level`, `lat`, `lon`,
/// `elevation`, `parent`, `tz` and `iso` (absent values are null), plus `score` for a search
/// and `meters` and `bearing` for a reverse lookup.
pub struct Engine {
    shared: Shared,
}

impl core::fmt::Debug for Engine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Engine")
            .field("layers", &self.layers())
            .finish()
    }
}

/// One thread's query buffers.
#[derive(Default)]
struct Buffers {
    scratch: Scratch,
    hits: Vec<Hit>,
    near: Vec<Near>,
    units: Vec<Place>,
    codes: Vec<PostalCode>,
}

thread_local! {
    /// Each thread's query scratch, reused so a warm query does not allocate (M1).
    static BUFFERS: RefCell<Buffers> = RefCell::default();
}

fn limit(n: u32) -> Result<usize> {
    if n > LIMIT_MAX {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("limit is {n}; at most {LIMIT_MAX}"),
        ));
    }
    Ok(n as usize)
}

fn point<G: teigeodata_core::Grid>(lat: f64, lon: f64) -> Result<Point<G>> {
    Point::from_degrees(lat, lon).map_err(|e| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("({lat}, {lon}) is not a point: {e}"),
        )
    })
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// An engine with no packs.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Shared::new(teigeodata_engine::Engine::new()),
        }
    }

    /// Opens the pack at `path` (memory-mapped) and adds it. Returns its layer ID, for
    /// [`remove`](Self::remove): unique in this engine, counting up from 0.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Io`] if the file cannot be read, [`ErrorCode::Corrupt`] if it is not a
    /// valid pack.
    pub fn open_file(&self, path: &str) -> Result<u32> {
        self.open_file_with(path, &OpenOptions::default())
    }

    /// Opens the pack at `path` with these options and adds it. Returns its layer ID, as
    /// [`open_file`](Self::open_file).
    ///
    /// # Errors
    ///
    /// As [`open_file`](Self::open_file), and [`ErrorCode::InvalidArgument`] for an outline
    /// level above 255.
    pub fn open_file_with(&self, path: &str, options: &OpenOptions) -> Result<u32> {
        let layer = Layer::open(path, options.layer()?)?;
        Ok(self.add(layer))
    }

    /// Adds a pack from its bytes (copied). Returns its layer ID, as
    /// [`open_file`](Self::open_file).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Corrupt`] if the bytes are not a valid pack.
    pub fn open_bytes(&self, bytes: &[u8]) -> Result<u32> {
        self.open_bytes_with(bytes, &OpenOptions::default())
    }

    /// Adds a pack from its bytes (copied) with these options. Returns its layer ID, as
    /// [`open_file`](Self::open_file).
    ///
    /// # Errors
    ///
    /// As [`open_bytes`](Self::open_bytes), and [`ErrorCode::InvalidArgument`] for an outline
    /// level above 255.
    pub fn open_bytes_with(&self, bytes: &[u8], options: &OpenOptions) -> Result<u32> {
        let layer = Layer::from_bytes(bytes, options.layer()?)?;
        Ok(self.add(layer))
    }

    fn add(&self, layer: Layer) -> u32 {
        self.shared.update(|e| e.add(layer).get())
    }

    /// Removes the pack with this layer ID; false if no pack has it. Queries already running
    /// finish on the snapshot they started with; the pack's memory is released after them.
    #[must_use]
    pub fn remove(&self, layer: u32) -> bool {
        self.shared.update(|e| {
            let id = e.layers().map(Layer::id).find(|id| id.get() == layer);
            id.and_then(|id| e.remove(id)).is_some()
        })
    }

    /// The number of packs.
    #[must_use]
    pub fn layers(&self) -> u32 {
        u32::try_from(self.shared.load().layers().len()).unwrap_or(u32::MAX)
    }

    /// The place with this ID (`tgd:np/1`), or no row; a retired ID gives its survivor.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `id` is not a place ID; [`ErrorCode::Corrupt`] if a
    /// section read is corrupt.
    pub fn get(&self, id: &str) -> Result<Blob> {
        let id = place_id(id)?;
        let engine = self.shared.load();
        let place = engine.get(id)?;
        Places::of(&engine, place)?.blob()
    }

    /// The place's parent as the packs answer for it (one row), or no row for a root or an
    /// unknown ID.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `id` is not a place ID; [`ErrorCode::Corrupt`] if a
    /// section read is corrupt.
    pub fn parent(&self, id: &str) -> Result<Blob> {
        let id = place_id(id)?;
        let engine = self.shared.load();
        let parent = match engine.get(id)? {
            Some(p) => engine.parent(&p)?,
            None => None,
        };
        Places::of(&engine, parent)?.blob()
    }

    /// The place's ancestors, parent first up to the country; no rows for a root or an
    /// unknown ID.
    ///
    /// # Errors
    ///
    /// As [`parent`](Self::parent).
    pub fn ancestors(&self, id: &str) -> Result<Blob> {
        self.related(id, teigeodata_engine::Engine::ancestors)
    }

    /// The place's children, by ID; no rows for a leaf or an unknown ID.
    ///
    /// # Errors
    ///
    /// As [`parent`](Self::parent).
    pub fn children(&self, id: &str) -> Result<Blob> {
        self.related(id, teigeodata_engine::Engine::children)
    }

    /// `ancestors` and `children`: the places `walk` gives for the place with this ID.
    fn related(
        &self,
        id: &str,
        walk: fn(
            &teigeodata_engine::Engine,
            &Place,
            &mut Vec<Place>,
        ) -> core::result::Result<(), teigeodata_engine::Error>,
    ) -> Result<Blob> {
        let id = place_id(id)?;
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            b.units.clear();
            if let Some(p) = engine.get(id)? {
                walk(&engine, &p, &mut b.units)?;
            }
            Places::of(&engine, b.units.iter().copied())?.blob()
        })
    }

    /// The places nearest the place with this ID, not counting it, nearest first, at most
    /// `limit`, with `meters` and `bearing`; no rows for an unknown ID.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `id` is not a place ID or `limit` exceeds
    /// [`LIMIT_MAX`]; [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn nearby(&self, id: &str, limit: u32) -> Result<Blob> {
        let (id, k) = (place_id(id)?, self::limit(limit)?);
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, near, .. } = b;
            near.clear();
            if let Some(p) = engine.get(id)? {
                engine.nearby(&p, k, scratch, near)?;
            }
            near_blob(&engine, near)
        })
    }

    /// Places matching `query`, best first, at most `limit`; "Name, Parent" ranks a named
    /// parent first. The `score` column compares only within one query.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `limit` exceeds [`LIMIT_MAX`];
    /// [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn search(&self, query: &str, limit: u32) -> Result<Blob> {
        self.search_with(query, limit, &SearchOptions::default())
    }

    /// [`search`](Self::search) ranked by `options`: near a focus point, or in strict match
    /// order.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `limit` exceeds [`LIMIT_MAX`], the focus is off the
    /// globe or has one coordinate only, or its scale is not positive;
    /// [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn search_with(&self, query: &str, limit: u32, options: &SearchOptions) -> Result<Blob> {
        let (k, opts) = (self::limit(limit)?, options.engine()?);
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, hits, .. } = b;
            engine.search_with(query, k, &opts, scratch, hits)?;
            let mut out = Places::of(&engine, hits.iter().map(|h| h.place))?;
            out.score = Some(hits.iter().map(|h| h.score).collect());
            out.blob()
        })
    }

    /// The places nearest the point, nearest first, at most `limit`, with `meters` and
    /// `bearing` (degrees from north) to each.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe or a `limit` above
    /// [`LIMIT_MAX`]; [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn reverse(&self, lat: f64, lon: f64, limit: u32) -> Result<Blob> {
        let (k, at) = (self::limit(limit)?, point::<E6>(lat, lon)?);
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, near, .. } = b;
            engine.reverse(at, k, scratch, near)?;
            near_blob(&engine, near)
        })
    }

    /// The places within `meters` of the point (great-circle), nearest first, at most `limit`,
    /// with `meters` and `bearing` to each.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe, a negative or non-finite
    /// radius, or a `limit` above [`LIMIT_MAX`]; [`ErrorCode::Corrupt`] if a section read is
    /// corrupt.
    pub fn within_radius(&self, lat: f64, lon: f64, meters: f64, limit: u32) -> Result<Blob> {
        let (k, at) = (self::limit(limit)?, point::<E6>(lat, lon)?);
        if !(meters.is_finite() && meters >= 0.0) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("radius is {meters} m; it must be finite and not negative"),
            ));
        }
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, near, .. } = b;
            engine.within_radius(at, meters, k, scratch, near)?;
            near_blob(&engine, near)
        })
    }

    /// The admin units holding the point, country first.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe; [`ErrorCode::Corrupt`] if a
    /// section read is corrupt.
    pub fn admin_at(&self, lat: f64, lon: f64) -> Result<Blob> {
        let at = point::<E7>(lat, lon)?;
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, units, .. } = b;
            engine.admin_at(at, scratch, units)?;
            Places::of(&engine, units.iter().copied())?.blob()
        })
    }

    /// The terrain height at the point, as one row: `meters` above the datum (bilinear between
    /// the DEM samples around it), `source` (the DEM product's key, null where the pack does
    /// not state it), `resolution_m` (the sample spacing north to south) and `datum`
    /// (`EGM2008`). No row where no pack has terrain there (the sea, outside every grid).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe; [`ErrorCode::Corrupt`] if a
    /// grid read is corrupt.
    pub fn elevation_at(&self, lat: f64, lon: f64) -> Result<Blob> {
        let at = point::<E7>(lat, lon)?;
        let engine = self.shared.load();
        let height = BUFFERS.with_borrow_mut(|b| engine.elevation_at(at, &mut b.scratch))?;
        let rows: Vec<_> = height.into_iter().collect();
        let source: Vec<&str> = rows
            .iter()
            .map(|h| h.source.map_or("", teigeodata_engine::DemSource::key))
            .collect();
        let stated: Vec<bool> = rows.iter().map(|h| h.source.is_some()).collect();
        let meters: Vec<f64> = rows.iter().map(|h| h.meters).collect();
        let spacing: Vec<f64> = rows.iter().map(|h| h.resolution_m).collect();
        let datum: Vec<&str> = rows.iter().map(|h| h.datum.name()).collect();
        let source_c = Column::Dict(&source);
        Blob::encode(&[
            ("meters", Column::F64(&meters)),
            ("source", Column::Nullable(&source_c, &stated)),
            ("resolution_m", Column::F64(&spacing)),
            ("datum", Column::Dict(&datum)),
        ])
        .map_err(|e| Error::new(ErrorCode::Internal, e.to_string()))
    }

    /// The places postal code `code` names, one row per code and place, with the code's
    /// `postal`, `scheme`, `code_kind`, `valid_from` and `valid_to` columns. `scheme` names
    /// one scheme (`np`, `gb`); empty means every scheme whose rule accepts the input. The
    /// code may be typed with or without separators, in any case, or in another script's
    /// digits. Retired codes are returned too, with their validity.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn postal_lookup(&self, code: &str, scheme: &str) -> Result<Blob> {
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            installed(engine.postal_lookup(code, scheme_of(scheme), &mut b.codes))?;
            postal_blob(&engine, &b.codes)
        })
    }

    /// Up to `limit` postal codes starting with `input`, with their places (autocomplete),
    /// in code order per scheme; `scheme` as in [`postal_lookup`](Self::postal_lookup).
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `limit` exceeds [`LIMIT_MAX`];
    /// [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn postal_prefix(&self, input: &str, scheme: &str, limit: u32) -> Result<Blob> {
        let k = self::limit(limit)?;
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            installed(engine.postal_prefix(input, scheme_of(scheme), k, &mut b.codes))?;
            postal_blob(&engine, &b.codes)
        })
    }

    /// The postal codes of the place with this ID; no rows for an unknown ID.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `id` is not a place ID; [`ErrorCode::Corrupt`] if a
    /// section read is corrupt.
    pub fn postal_codes_of(&self, id: &str) -> Result<Blob> {
        let id = place_id(id)?;
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            b.codes.clear();
            if let Some(p) = engine.get(id)? {
                installed(engine.postal_codes_of(&p, &mut b.codes))?;
            }
            postal_blob(&engine, &b.codes)
        })
    }

    /// The postal codes of the point: those of the deepest admin unit holding it that an
    /// admin-aligned scheme gives codes to, with its parts (a ward's code; the point is in one
    /// of them). No rows where no unit holding the point has such codes.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe; [`ErrorCode::Corrupt`] if a
    /// section read is corrupt.
    pub fn postal_at(&self, lat: f64, lon: f64) -> Result<Blob> {
        let at = point::<E7>(lat, lon)?;
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, codes, .. } = b;
            installed(engine.postal_at(at, scratch, codes).map(|_| ()))?;
            postal_blob(&engine, codes)
        })
    }

    /// The time-zone area holding the point (one row; its `tz` column is the IANA zone), or no
    /// row where no pack answers.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe; [`ErrorCode::Corrupt`] if a
    /// section read is corrupt.
    pub fn tz_at(&self, lat: f64, lon: f64) -> Result<Blob> {
        let at = point::<E7>(lat, lon)?;
        let engine = self.shared.load();
        let area = engine.tz_at(at)?;
        Places::of(&engine, area)?.blob()
    }

    /// The place whose ISO 3166 code is `code` (`NP`, `NP-P3`, either case), or no row.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] if `code` is not an ISO 3166 code;
    /// [`ErrorCode::Corrupt`] if a section read is corrupt.
    pub fn by_iso(&self, code: &str) -> Result<Blob> {
        let code: teigeodata_core::IsoCode = code.parse().map_err(|e| {
            Error::new(
                ErrorCode::InvalidArgument,
                format!("{code:?} is not an ISO 3166 code: {e}"),
            )
        })?;
        let engine = self.shared.load();
        let place = engine.by_iso(code)?;
        Places::of(&engine, place)?.blob()
    }
}

fn place_id(id: &str) -> Result<PlaceId> {
    id.parse().map_err(|e| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("{id:?} is not a place ID: {e}"),
        )
    })
}

/// An empty scheme is every scheme.
fn scheme_of(scheme: &str) -> Option<&str> {
    (!scheme.is_empty()).then_some(scheme)
}

/// A pack without postal codes installed answers no codes rather than an error (D-133).
fn installed(r: core::result::Result<(), teigeodata_engine::Error>) -> Result<()> {
    match r {
        Err(teigeodata_engine::Error::MissingComponent { .. }) | Ok(()) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// A blob of `codes` with their places.
fn postal_blob(engine: &teigeodata_engine::Engine, codes: &[PostalCode]) -> Result<Blob> {
    let mut out = Places::of(engine, codes.iter().map(|c| c.place))?;
    out.postal = Some(places::Postal::of(codes));
    out.blob()
}

/// A blob of `near`'s places with their `meters` and `bearing`.
fn near_blob(engine: &teigeodata_engine::Engine, near: &[Near]) -> Result<Blob> {
    let mut out = Places::of(engine, near.iter().map(|n| n.place))?;
    out.near = Some((
        near.iter().map(|n| n.meters).collect(),
        near.iter().map(|n| n.bearing_deg).collect(),
    ));
    out.blob()
}

/// What [`Store::install`] takes of each pack. Every field's zero value is its default
/// (D-276): the whole pack.
#[cfg(feature = "store")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallOptions {
    /// The components, by name and comma-separated (`point,names,search`); what they need is
    /// added. Empty for every component.
    pub components: String,
    /// The languages besides the base names, as comma-separated BCP 47 tags (`en,ne`), at
    /// most 8. Empty for every language.
    pub languages: String,
    /// The deepest admin level of the one outline part installed; `None` for every outline.
    pub outline_level: Option<u32>,
    /// The tolerance in metres of that outline part; 0 is full resolution.
    pub outline_tolerance_m: u32,
    /// Keeps the packs when room is made.
    pub pin: bool,
}

/// A store of installed packs in one directory: its signed catalog, its packs and their
/// journal (D-171), shared by every language and the command line (research note
/// store-api). Long work runs as a [`Job`] on a thread of its own; one store runs its jobs
/// one at a time, and its other calls wait for the job running.
///
/// Packs are named as catalog paths (`np.places.tgd`) or short (`np`, `np.dem`), several
/// comma-separated. Results are blobs:
/// - [`catalog`](Self::catalog): `path`, `release`, `download`, `disk`, `components`,
///   `licence`, `state` (`available`, `installed` or `update`);
/// - [`status`](Self::status): `path`, `release`, `bytes`, `pinned`, `asked`, `health`
///   (`good`, `length`, `head`, `footer` or `sections`), `components`;
/// - an install or update: `path`, `release`, `fetched`, `reused`, `kept`, `changed`;
/// - a refresh: `timestamp`, `files`, `bytes`; a repair: `path`, `health`, `fetched`;
///   [`remove`](Self::remove): `path`, `release`;
/// - [`find`](Self::find) and [`locate`](Self::locate): one row per place and pack that
///   covers it: `id`, `name`, `lat`, `lon`, `countries`, then the pack's `pack`, `release`,
///   `download`, `disk` and `state`, null for a place no pack in the catalog covers.
#[cfg(feature = "store")]
pub struct Store {
    inner: Arc<store::Inner>,
}

#[cfg(feature = "store")]
impl core::fmt::Debug for Store {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

#[cfg(feature = "store")]
impl Store {
    /// Sets up a store at `root` and opens it: the metadata mirrors `repo` and asset mirrors
    /// `assets` (comma-separated URLs), and `trusted_root`, the catalog's root metadata, which
    /// becomes its trust anchor once it verifies by itself. A store set up before is set up
    /// again with these.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Refused`] if the root does not verify or the store cannot be written.
    pub fn create(root: &str, repo: &str, assets: &str, trusted_root: &[u8]) -> Result<Self> {
        let list = |s: &str| -> Vec<String> {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_owned)
                .collect()
        };
        teigeodata_sync::client::init(root.as_ref(), &list(repo), &list(assets), trusted_root)?;
        Self::open(root)
    }

    /// Opens the store at `root`, finishing or undoing an install a crash cut short.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Refused`] if it is not a store or its journal cannot be read.
    pub fn open(root: &str) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(store::Inner::open(root)?),
        })
    }

    /// The packs found damaged when the store was opened, comma-separated; empty when none.
    /// [`repair`](Self::repair) fetches what they lack.
    #[must_use]
    pub fn damaged(&self) -> String {
        self.inner.damaged.clone()
    }

    /// The verified catalog as stored, one row per pack, with its state against the store.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Refused`] before a refresh, or if the stored catalog no longer verifies.
    pub fn catalog(&self) -> Result<Blob> {
        store::catalog(&mut self.inner.client())
    }

    /// Each installed pack and its health, from a full check of every byte when `deep`.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Refused`] if a pack cannot be read.
    pub fn status(&self, deep: bool) -> Result<Blob> {
        store::status(&mut self.inner.client(), deep)
    }

    /// Pins the pack (kept when room is made), or unpins it.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Refused`] if it is not installed.
    pub fn pin(&self, pack: &str, on: bool) -> Result<()> {
        let mut c = self.inner.client();
        for p in store::paths(pack) {
            c.pin(&p, on)?;
        }
        Ok(())
    }

    /// Removes the packs and the dependencies nothing else needs; returns what was removed.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Refused`] if one is not installed or another pack needs it.
    pub fn remove(&self, pack: &str) -> Result<Blob> {
        let mut c = self.inner.client();
        let mut gone = Vec::new();
        for p in store::paths(pack) {
            gone.extend(c.remove(&p)?);
        }
        store::removed(&gone)
    }

    /// The places named `query` in `engine` (one over a world pack the app ships, the
    /// installed packs, or both), best first, each with the catalog's packs that cover it: what
    /// to install for "Dubai". Runs offline on the stored catalog.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a limit above [`LIMIT_MAX`];
    /// [`ErrorCode::Refused`] if the stored catalog no longer verifies.
    pub fn find(&self, engine: &Engine, query: &str, limit: u32) -> Result<Blob> {
        let limit = crate::limit(limit)?;
        store::find(
            &mut self.inner.client(),
            &engine.shared.load(),
            query,
            limit,
        )
    }

    /// The countries within `radius_m` metres of a point in `engine`, each with the catalog's
    /// packs that cover the point: what to install for where the user stands.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for a point off the globe; [`ErrorCode::Refused`] if
    /// the stored catalog no longer verifies.
    pub fn locate(&self, engine: &Engine, lat: f64, lon: f64, radius_m: u32) -> Result<Blob> {
        let at = point(lat, lon)?;
        store::locate(
            &mut self.inner.client(),
            &engine.shared.load(),
            at,
            radius_m,
        )
    }

    /// An engine over the installed packs, each opened with the components and languages
    /// installed.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Io`] or [`ErrorCode::Corrupt`] if a pack cannot be opened.
    pub fn engine(&self) -> Result<Engine> {
        store::engine(&self.inner.client())
    }

    /// Refreshes the catalog for the countries (comma-separated ISO codes) and every country
    /// installed. Fails with [`ErrorCode::Network`] or [`ErrorCode::Refused`].
    #[must_use]
    pub fn refresh(&self, countries: &str) -> Job {
        let countries: Vec<String> = countries
            .split(',')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_owned)
            .collect();
        self.job(move |c| store::refresh(c, &countries))
    }

    /// Installs the packs with `options`, dependencies first, refreshing the catalog for their
    /// countries. Cancelled, it leaves the store as it was; the packs installed before the
    /// one cut stay installed.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::InvalidArgument`] for an unknown component or language tag, more than 8
    /// languages, or an outline level above 255; the job's own errors are the job's.
    pub fn install(&self, packs: &str, options: &InstallOptions) -> Result<Job> {
        let paths = store::paths(packs);
        let want = store::want(options)?;
        let pin = options.pin;
        Ok(self.job(move |c| store::done(&c.install(&paths, &want, pin, None)?.done)))
    }

    /// Updates the packs (every installed one when empty) to the catalog's releases, keeping
    /// the components and languages installed.
    #[must_use]
    pub fn update(&self, packs: &str) -> Job {
        let paths = store::paths(packs);
        self.job(move |c| store::done(&c.update(&paths)?))
    }

    /// Checks the pack in full and, unless it is whole, fetches what it lacks.
    #[must_use]
    pub fn repair(&self, pack: &str) -> Job {
        let path = teigeodata_sync::client::path_of(pack.trim());
        self.job(move |c| store::repaired(c, &path))
    }

    fn job(
        &self,
        work: impl FnOnce(&mut teigeodata_sync::client::Client<store::Net>) -> Result<Blob>
        + Send
        + 'static,
    ) -> Job {
        Job {
            inner: store::spawn(self.inner.clone(), work),
        }
    }
}

/// A store's long work on a thread of its own (D-274): its progress in bytes, a wait for the
/// next change, a cancel, and its result.
#[cfg(feature = "store")]
pub struct Job {
    inner: Arc<store::JobInner>,
}

#[cfg(feature = "store")]
impl core::fmt::Debug for Job {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Job")
            .field("progress", &self.inner.watch.progress())
            .finish()
    }
}

#[cfg(feature = "store")]
impl Job {
    /// Waits until the progress moves past `seq` (from [`seq`](Self::seq)), the job ends, or
    /// `timeout_ms` passes; returns the sequence then. Polling with the last one returned
    /// sees every change.
    #[must_use]
    pub fn wait_progress(&self, seq: u64, timeout_ms: u32) -> u64 {
        self.inner
            .watch
            .wait(
                seq,
                core::time::Duration::from_millis(u64::from(timeout_ms)),
            )
            .seq
    }

    /// The progress's sequence number: it moves on with every change.
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.inner.watch.progress().seq
    }

    /// Bytes received so far.
    #[must_use]
    pub fn done_bytes(&self) -> u64 {
        self.inner.watch.progress().done
    }

    /// Bytes expected, from the catalog's sizes: never below the bytes received, and equal
    /// to them once the job ends.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.inner.watch.progress().total
    }

    /// Whether the job has ended (done, failed or cancelled); its result is then ready.
    #[must_use]
    pub fn is_ended(&self) -> bool {
        self.inner.watch.progress().ended
    }

    /// Asks the job to stop at its next request or chunk; it then fails with
    /// [`ErrorCode::Cancelled`] and leaves the store as it was.
    pub fn cancel(&self) {
        self.inner.watch.cancel();
    }

    /// The job's result, waiting for it to end.
    ///
    /// # Errors
    ///
    /// The job's: [`ErrorCode::Network`], [`ErrorCode::Refused`], [`ErrorCode::Cancelled`],
    /// or [`ErrorCode::Io`] and [`ErrorCode::Corrupt`] for an engine's packs.
    pub fn result(&self) -> Result<Blob> {
        self.inner.result()
    }
}

/// The library version, from its package metadata.
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Does nothing. Measures the bare cost of a call across each boundary.
pub fn noop() {}

/// The search fold of a name: NFKD, lower case, optional accents dropped by script, spaces
/// collapsed, recomposed to NFC. The same fold the forge and the search index use.
#[must_use]
pub fn fold(text: &str) -> String {
    teigeodata_names::fold(text)
}

/// Builds `rows` deterministic sample places as a blob with the columns `id` (`u32`), `lat`
/// and `lon` (`f64`, degrees), `elevation` (`f32`, metres) and `name` (UTF-8). Exercises the
/// result path of every binding.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] if `rows` exceeds [`SAMPLE_ROWS_MAX`].
pub fn sample_rows(rows: u32) -> Result<Blob> {
    if rows > SAMPLE_ROWS_MAX {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("rows is {rows}; at most {SAMPLE_ROWS_MAX}"),
        ));
    }
    let n = rows as usize;
    let (mut id, mut lat, mut lon, mut elevation) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    let mut names = String::with_capacity(n * 12);
    let mut ends = Vec::with_capacity(n);
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for i in 0..rows {
        // SplitMix64: deterministic, so every language checks the same values.
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let unit =
            |bits: u64| f64::from(u32::try_from(bits & 0xffff_ffff).unwrap_or(0)) / 4_294_967_296.0;
        id.push(i);
        lat.push(unit(z) * 180.0 - 90.0);
        lon.push(unit(z >> 32) * 360.0 - 180.0);
        elevation.push(f32::from(u16::try_from(z >> 48).unwrap_or(0)) / 8.0 - 500.0);
        names.push_str("place ");
        push_decimal(&mut names, i);
        ends.push(names.len());
    }
    let mut start = 0;
    let name: Vec<&str> = ends
        .iter()
        .map(|&end| {
            let s = names.get(start..end).unwrap_or_default();
            start = end;
            s
        })
        .collect();
    Blob::encode(&[
        ("id", Column::U32(&id)),
        ("lat", Column::F64(&lat)),
        ("lon", Column::F64(&lon)),
        ("elevation", Column::F32(&elevation)),
        ("name", Column::Utf8(&name)),
    ])
    .map_err(|e| Error::new(ErrorCode::Internal, e.to_string()))
}

/// Appends `v` in decimal without allocating.
fn push_decimal(out: &mut String, mut v: u32) {
    let mut digits = [0u8; 10];
    let mut start = digits.len();
    for slot in digits.iter_mut().rev() {
        // `v % 10` < 10, so the cast cannot truncate.
        *slot = b'0' + (v % 10) as u8;
        start -= 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.push_str(core::str::from_utf8(digits.get(start..).unwrap_or_default()).unwrap_or_default());
}

/// Calls `on_step` once per step with the step number, from 1 to `steps`, on the calling
/// thread, and returns the number of steps run. Exercises callbacks in every binding.
///
/// # Errors
///
/// [`ErrorCode::Cancelled`] if `on_step` returns `false`.
pub fn progress(steps: u32, on_step: &mut dyn FnMut(u32) -> bool) -> Result<u32> {
    for step in 1..=steps {
        if !on_step(step) {
            return Err(Error::new(
                ErrorCode::Cancelled,
                format!("cancelled at step {step} of {steps}"),
            ));
        }
    }
    Ok(steps)
}

/// Mean Earth radius of WGS84, metres (IUGG R1 = (2a + b) / 3).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Great-circle distance in metres between two points in degrees, by the haversine formula on
/// the WGS84 mean sphere (error up to about 0.5% against the ellipsoid).
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] if a latitude is outside -90..=90, a longitude outside
/// -180..=180, or any value is not finite.
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Result<f64> {
    for (name, v, max) in [
        ("lat1", lat1, 90.0),
        ("lon1", lon1, 180.0),
        ("lat2", lat2, 90.0),
        ("lon2", lon2, 180.0),
    ] {
        if !(-max..=max).contains(&v) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{name} is {v}; it must be within ±{max}"),
            ));
        }
    }
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    Ok(2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin())
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, SAMPLE_ROWS_MAX, fold, progress, sample_rows, version};
    use teigeodata_blob::{Reader, View};

    #[test]
    fn distance_is_haversine_and_checks_ranges() {
        // Kathmandu to Pokhara: 142.4 km on the sphere.
        let d = super::distance_m(27.7172, 85.3240, 28.2096, 83.9856).unwrap();
        assert!((d - 142_394.0).abs() < 1.0, "{d}");
        assert_eq!(super::distance_m(1.0, 2.0, 1.0, 2.0), Ok(0.0));
        let half = super::distance_m(0.0, 0.0, 0.0, 180.0).unwrap();
        assert!((half - core::f64::consts::PI * super::EARTH_RADIUS_M).abs() < 1e-6);
        for bad in [f64::NAN, 91.0, -90.5, f64::INFINITY] {
            let e = super::distance_m(bad, 0.0, 0.0, 0.0).unwrap_err();
            assert_eq!(e.code, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn version_is_the_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn fold_is_the_names_fold() {
        assert_eq!(fold("  Lahachowk  BAZAR "), "lahachowk bazar");
        assert_eq!(fold("São Paulo"), teigeodata_names::fold("São Paulo"));
    }

    #[test]
    fn sample_rows_are_deterministic_and_in_range() {
        let blob = sample_rows(1000).unwrap();
        assert_eq!(blob, sample_rows(1000).unwrap());
        let r = Reader::new(blob.as_bytes()).unwrap();
        assert_eq!(r.rows(), 1000);
        let Some(View::F64(lat)) = r.column("lat") else {
            panic!()
        };
        let Some(View::F64(lon)) = r.column("lon") else {
            panic!()
        };
        let Some(View::F32(e)) = r.column("elevation") else {
            panic!()
        };
        let Some(View::Utf8(name)) = r.column("name") else {
            panic!()
        };
        assert!(lat.f64s().all(|v| (-90.0..90.0).contains(&v)));
        assert!(lon.f64s().all(|v| (-180.0..180.0).contains(&v)));
        assert!(e.f32s().all(|v| (-500.0..7692.0).contains(&v)));
        assert_eq!(name.get(999), Some("place 999"));
        // The parity values every binding checks (bindings/parity.json): sums in row order.
        assert_eq!(lat.f64s().sum::<f64>(), -1_116.085_741_496_645);
        assert_eq!(e.f32s().map(f64::from).sum::<f64>(), 3_528_269.125);
    }

    #[test]
    fn sample_rows_has_a_limit() {
        assert_eq!(sample_rows(0).map(|b| !b.as_bytes().is_empty()), Ok(true));
        let e = sample_rows(SAMPLE_ROWS_MAX + 1).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn progress_calls_back_and_cancels() {
        let mut seen = Vec::new();
        assert_eq!(
            progress(3, &mut |s| {
                seen.push(s);
                true
            }),
            Ok(3)
        );
        assert_eq!(seen, [1, 2, 3]);
        let e = progress(10, &mut |s| s < 4).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::Cancelled, "cancelled at step 4 of 10")
        );
        assert_eq!(progress(0, &mut |_| false), Ok(0));
    }

    /// `bindings/parity.json` holds the expectations of every language's parity test; this
    /// checks them against Rust, so a binding that passes agrees with the core.
    #[test]
    fn parity_file_matches_rust() {
        let parity: serde_json::Value =
            serde_json::from_str(include_str!("../../../bindings/parity.json")).unwrap();
        assert_eq!(parity["version"], version());
        for case in parity["fold"].as_array().unwrap() {
            assert_eq!(fold(case[0].as_str().unwrap()), case[1].as_str().unwrap());
        }
        let rows = &parity["sample_rows"];
        let blob = sample_rows(u32::try_from(rows["rows"].as_u64().unwrap()).unwrap()).unwrap();
        assert_eq!(
            blob.as_bytes().len() as u64,
            rows["bytes"].as_u64().unwrap()
        );
        let reader = Reader::new(blob.as_bytes()).unwrap();
        let columns: Vec<_> = reader.names().collect();
        assert_eq!(serde_json::json!(columns), rows["columns"]);
        let Some(View::F64(lat)) = reader.column("lat") else {
            panic!()
        };
        assert_eq!(lat.f64s().sum::<f64>(), rows["lat_sum"].as_f64().unwrap());
        let Some(View::F32(elev)) = reader.column("elevation") else {
            panic!()
        };
        let elev: f64 = elev.f32s().map(f64::from).sum();
        assert_eq!(elev, rows["elevation_sum"].as_f64().unwrap());
        let Some(View::Utf8(names)) = reader.column("name") else {
            panic!()
        };
        assert_eq!(names.get(reader.rows() - 1), rows["last_name"].as_str());
        assert_eq!(parity["sample_rows_max"], SAMPLE_ROWS_MAX);
        let prog = &parity["progress"];
        let (steps, after) = (
            prog["steps"].as_u64().unwrap(),
            prog["cancel_after"].as_u64().unwrap(),
        );
        let err = progress(u32::try_from(steps).unwrap(), &mut |step| {
            u64::from(step) < after
        })
        .unwrap_err();
        assert_eq!(err.message, prog["cancelled_message"].as_str().unwrap());
        let d = &parity["distance_m"];
        let args: Vec<f64> = d["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let got = super::distance_m(args[0], args[1], args[2], args[3]).unwrap();
        assert!((got - d["metres"].as_f64().unwrap()).abs() < 1e-6, "{got}");
        let codes = &parity["error_codes"];
        for (code, name) in [
            (ErrorCode::InvalidArgument, "InvalidArgument"),
            (ErrorCode::Cancelled, "Cancelled"),
            (ErrorCode::Internal, "Internal"),
        ] {
            assert_eq!(codes[name], code as i32);
        }
    }
}
