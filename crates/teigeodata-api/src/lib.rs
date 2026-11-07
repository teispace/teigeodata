//! The safe public API of teigeodata.
//!
//! This crate is the single source of every language binding ([ADR-0016]). `cargo xtask
//! bindings` reads the `pub fn`s below with `syn`, writes `bindings/idl.json` and generates the
//! C ABI, its header and the Swift, Dart, Python, Node and JVM layers from it. A function
//! added here appears in every language after one `cargo xtask bindings`.
//!
//! **Boundary types.** A public function may use only these, so that every language can
//! express it:
//! - parameters: `bool`, `i32`, `u32`, `i64`, `u64`, `f64`, `&str`, and one callback
//!   `&mut dyn FnMut(u32) -> bool` (return `false` to cancel);
//! - results: `()`, the scalars, `String` and [`Blob`], optionally inside [`Result`].
//!
//! The extractor rejects anything else with the line number.
//!
//! [ADR-0016]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0016-bindings.md
//!
//! ```
//! assert_eq!(teigeodata_api::fold("São Paulo"), "sao paulo");
//! ```

pub use teigeodata_blob::Blob;
use teigeodata_blob::Column;

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

/// The result of a fallible call.
pub type Result<T> = core::result::Result<T, Error>;

/// Most rows [`sample_rows`] builds in one call.
pub const SAMPLE_ROWS_MAX: u32 = 1_000_000;

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
        names.push_str(&i.to_string());
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

#[cfg(test)]
mod tests {
    use super::{ErrorCode, SAMPLE_ROWS_MAX, fold, progress, sample_rows, version};
    use teigeodata_blob::{Reader, View};

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
