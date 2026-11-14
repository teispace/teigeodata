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
//! The extractor rejects anything else with the line number.
//!
//! [ADR-0016]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0016-bindings.md
//!
//! ```
//! assert_eq!(teigeodata_api::fold("São Paulo"), "sao paulo");
//! ```

mod places;

use std::cell::RefCell;

pub use teigeodata_blob::Blob;
use teigeodata_blob::Column;
use teigeodata_core::{E6, E7, PlaceId, Point};
use teigeodata_engine::{Hit, Layer, LayerOptions, Near, Place, Scratch, Shared};

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
            // A missing component reads as absent where a query allows it; elsewhere it is
            // the caller asking for what was not installed.
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

    /// Opens the pack at `path` (memory-mapped) and adds it. Returns the number of packs.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Io`] if the file cannot be read, [`ErrorCode::Corrupt`] if it is not a
    /// valid pack.
    pub fn open_file(&self, path: &str) -> Result<u32> {
        let layer = Layer::open(path, LayerOptions::default())?;
        Ok(self.add(layer))
    }

    /// Adds a pack from its bytes (copied). Returns the number of packs.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Corrupt`] if the bytes are not a valid pack.
    pub fn open_bytes(&self, bytes: &[u8]) -> Result<u32> {
        let layer = Layer::from_bytes(bytes, LayerOptions::default())?;
        Ok(self.add(layer))
    }

    fn add(&self, layer: Layer) -> u32 {
        self.shared.update(|e| {
            e.add(layer);
            u32::try_from(e.layers().len()).unwrap_or(u32::MAX)
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
        let k = self::limit(limit)?;
        let engine = self.shared.load();
        BUFFERS.with_borrow_mut(|b| {
            let Buffers { scratch, hits, .. } = b;
            engine.search(query, k, scratch, hits)?;
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

/// A blob of `near`'s places with their `meters` and `bearing`.
fn near_blob(engine: &teigeodata_engine::Engine, near: &[Near]) -> Result<Blob> {
    let mut out = Places::of(engine, near.iter().map(|n| n.place))?;
    out.near = Some((
        near.iter().map(|n| n.meters).collect(),
        near.iter().map(|n| n.bearing_deg).collect(),
    ));
    out.blob()
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
