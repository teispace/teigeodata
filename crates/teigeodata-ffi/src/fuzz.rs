//! The C ABI's fuzz harness (feature `fuzz`, D-331).
//!
//! [`abi`] opens the seed packs through `tg_engine_open_bytes` and calls every `Engine`
//! function of the ABI with arguments from the fuzz bytes:
//! - strings that are not UTF-8, hold NUL bytes, are empty with a null pointer or name real IDs
//!   and zones;
//! - coordinates that are NaN, infinite or past the poles;
//! - limits up to `u32::MAX`;
//! - option records with any field set;
//! - layer numbers that do not exist.
//!
//! The property is the ABI's contract: no call panics (a stopped panic returns
//! `TG_ERROR_INTERNAL` with its own message, which fails the harness), and every buffer it
//! hands out is freed with `tg_bytes_free`. Store and job functions touch the file system and
//! the network and are left out. `tests/fuzz.rs` fails when the IDL gains an `Engine` method
//! this harness does not call.

#![allow(
    unsafe_code,
    clippy::missing_panics_doc,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "a test harness of the C ABI: it calls the exported functions as C does, a panic \
              is how a property failure is reported, and indexes are taken modulo the length"
)]

use std::vec::Vec;

use crate::abi::{TgBytes, tg_bytes_free, tg_last_error};
use crate::generated::{
    TgEngine, TgOpenOptions, TgSearchOptions, tg_engine_admin_at, tg_engine_ancestors,
    tg_engine_boundary, tg_engine_by_iso, tg_engine_children, tg_engine_distance,
    tg_engine_elevation_at, tg_engine_free, tg_engine_get, tg_engine_layers, tg_engine_nearby,
    tg_engine_new, tg_engine_open_bytes, tg_engine_open_bytes_with, tg_engine_open_file,
    tg_engine_open_file_range, tg_engine_open_file_range_with, tg_engine_open_file_with,
    tg_engine_parent, tg_engine_postal_at, tg_engine_postal_codes_of, tg_engine_postal_lookup,
    tg_engine_postal_prefix, tg_engine_remove, tg_engine_reverse, tg_engine_search,
    tg_engine_search_with, tg_engine_tz_at, tg_engine_tz_local, tg_engine_tz_offset_at,
    tg_engine_tz_rules_start, tg_engine_tz_version, tg_engine_tz_zones, tg_engine_within_radius,
};

/// Fuzz bytes as values; runs out into zeros.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> u8 {
        let (v, rest) = self.0.split_first().map_or((0, &[][..]), |(v, r)| (*v, r));
        self.0 = rest;
        v
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes([0; 4].map(|_| self.u8()))
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes([0; 8].map(|_| self.u8()))
    }
    fn bytes(&mut self, max: usize) -> &'a [u8] {
        let n = usize::from(self.u8()) % (max + 1);
        let (v, rest) = self.0.split_at(n.min(self.0.len()));
        self.0 = rest;
        v
    }
}

/// Real values of the seeds, so calls get past argument checks and reach the queries.
const KNOWN: [&[u8]; 6] = [
    b"tgd:np/1rxre8",
    b"tgd:np/1",
    b"Asia/Kathmandu",
    b"NP",
    b"44600",
    b"Pokhara",
];

/// A string argument: owned bytes, and whether to pass it as a null pointer with its length.
struct Text {
    bytes: Vec<u8>,
    null: bool,
}

impl Text {
    fn from(c: &mut Cursor<'_>) -> Self {
        let pick = c.u8();
        let bytes = match pick % 8 {
            0..=2 => c.bytes(48).to_vec(),
            3 => {
                let mut b = KNOWN[usize::from(c.u8()) % KNOWN.len()].to_vec();
                b.push(0);
                b.extend_from_slice(c.bytes(4));
                b
            }
            _ => KNOWN[usize::from(c.u8()) % KNOWN.len()].to_vec(),
        };
        // A null pointer is allowed only with length 0; with a length it must be refused.
        Self {
            bytes,
            null: pick & 0x80 != 0,
        }
    }
    fn ptr(&self) -> *const u8 {
        if self.null || self.bytes.is_empty() {
            core::ptr::null()
        } else {
            self.bytes.as_ptr()
        }
    }
    fn len(&self) -> usize {
        self.bytes.len()
    }
}

/// A coordinate: often near the seeds' places, else anywhere, NaN, infinite or out of range.
fn coord(c: &mut Cursor<'_>, base: f64) -> f64 {
    match c.u8() % 6 {
        0 => f64::from_bits(c.u64()),
        1 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308, -0.0][usize::from(c.u8()) % 5],
        2 => f64::from(c.u32()) / 1e6 - 2_000.0,
        _ => base + f64::from(c.u8()) / 32.0 - 4.0,
    }
}

/// A limit: usually small, sometimes huge.
fn limit(c: &mut Cursor<'_>) -> u32 {
    match c.u8() % 8 {
        0 => u32::MAX,
        1 => c.u32(),
        _ => u32::from(c.u8() % 32),
    }
}

/// Checks a call's status: anything but a stopped panic.
fn status(call: &str, code: i32) {
    if code == teigeodata_api::ErrorCode::Internal as i32 {
        let mut m = TgBytes::EMPTY;
        // SAFETY: a writable out pointer.
        let _ = unsafe { tg_last_error(&raw mut m) };
        let text = if m.ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: a buffer the library handed out, of `len` bytes.
            unsafe { core::slice::from_raw_parts(m.ptr, m.len) }.to_vec()
        };
        // SAFETY: a buffer the library handed out, freed once.
        unsafe { tg_bytes_free(m) };
        let text = String::from_utf8_lossy(&text);
        assert!(!text.contains("panic"), "{call}: {text}");
    }
}

/// Frees a buffer a call wrote (empty when it failed).
fn free(out: &mut TgBytes) {
    let b = core::mem::replace(out, TgBytes::EMPTY);
    // SAFETY: a buffer the library handed out, or the empty one; freed once.
    unsafe { tg_bytes_free(b) };
}

/// A blob-returning call's status checked, its buffer freed.
macro_rules! blob {
    ($name:literal, |$out:ident| $call:expr) => {{
        let mut buf = TgBytes::EMPTY;
        let $out = &raw mut buf;
        // SAFETY: the arguments are live for the call: buffers owned here, the handle open.
        status($name, unsafe { $call });
        free(&mut buf);
    }};
}

/// Every `Engine` function of the C ABI on `seeds`, with arguments from `data`.
pub fn abi(seeds: &[&[u8]], data: &[u8]) {
    let mut c = Cursor(data);
    let mut e: *mut TgEngine = core::ptr::null_mut();
    // SAFETY: a writable out pointer.
    status("new", unsafe { tg_engine_new(&raw mut e) });
    assert!(!e.is_null(), "tg_engine_new gave no handle");
    let h = e.cast_const();
    let mut layer = 0_u32;
    for seed in seeds {
        // SAFETY: the seed's bytes are live for the call.
        status("open_bytes", unsafe {
            tg_engine_open_bytes(h, seed.as_ptr(), seed.len(), &raw mut layer)
        });
    }

    let options = |c: &mut Cursor<'_>, comp: &Text, lang: &Text| TgOpenOptions {
        priority: c.u32().cast_signed(),
        verify: u32::from(c.u8() % 4),
        load: u32::from(c.u8() % 3),
        has_outline_level: c.u8() & 1 == 1,
        outline_level: c.u32() % 300,
        outline_tolerance_m: c.u32(),
        components: comp.ptr(),
        components_len: comp.len(),
        languages: lang.ptr(),
        languages_len: lang.len(),
    };
    let (comp, lang) = (Text::from(&mut c), Text::from(&mut c));
    let o = options(&mut c, &comp, &lang);
    if let Some(seed) = seeds.get(usize::from(c.u8()) % seeds.len().max(1)) {
        // SAFETY: the seed and the option record's strings are live for the call.
        status("open_bytes_with", unsafe {
            tg_engine_open_bytes_with(h, seed.as_ptr(), seed.len(), &raw const o, &raw mut layer)
        });
    }
    // Files: a path that cannot exist (hex of the fuzz bytes, so no `..` reaches a real file),
    // or a string that is not UTF-8.
    let raw = Text::from(&mut c);
    let path: Vec<u8> = if c.u8() & 1 == 0 {
        let hex: String = raw.bytes.iter().fold(String::new(), |mut s, b| {
            use core::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        });
        format!("/nonexistent/teigeodata-fuzz/{hex}.tgd").into_bytes()
    } else {
        vec![0xff, 0xfe, b'/', 0]
    };
    let (off, len) = (c.u64(), c.u64());
    // SAFETY: the path and options are live for each call.
    unsafe {
        status(
            "open_file",
            tg_engine_open_file(h, path.as_ptr(), path.len(), &raw mut layer),
        );
        status(
            "open_file_with",
            tg_engine_open_file_with(h, path.as_ptr(), path.len(), &raw const o, &raw mut layer),
        );
        status(
            "open_file_range",
            tg_engine_open_file_range(h, path.as_ptr(), path.len(), off, len, &raw mut layer),
        );
        status(
            "open_file_range_with",
            tg_engine_open_file_range_with(
                h,
                path.as_ptr(),
                path.len(),
                off,
                len,
                &raw const o,
                &raw mut layer,
            ),
        );
    }

    // The queries, in an order the fuzz bytes pick, until they run out.
    for _ in 0..12 {
        let (a, b) = (Text::from(&mut c), Text::from(&mut c));
        let (lat, lon) = (coord(&mut c, 28.2), coord(&mut c, 84.0));
        let n = limit(&mut c);
        match c.u8() % 27 {
            0 => blob!("get", |o| tg_engine_get(h, a.ptr(), a.len(), o)),
            1 => blob!("parent", |o| tg_engine_parent(h, a.ptr(), a.len(), o)),
            2 => blob!("ancestors", |o| tg_engine_ancestors(h, a.ptr(), a.len(), o)),
            3 => blob!("children", |o| tg_engine_children(h, a.ptr(), a.len(), o)),
            4 => blob!("nearby", |o| tg_engine_nearby(h, a.ptr(), a.len(), n, o)),
            5 => blob!("search", |o| tg_engine_search(h, a.ptr(), a.len(), n, o)),
            6 => {
                let s = TgSearchOptions {
                    has_focus_lat: c.u8() & 1 == 1,
                    focus_lat: lat,
                    has_focus_lon: c.u8() & 1 == 1,
                    focus_lon: lon,
                    has_focus_scale_m: c.u8() & 1 == 1,
                    focus_scale_m: coord(&mut c, 50_000.0),
                    strict: c.u8() & 1 == 1,
                };
                let opts: *const TgSearchOptions = if c.u8() % 4 == 0 {
                    core::ptr::null()
                } else {
                    &raw const s
                };
                blob!("search_with", |o| tg_engine_search_with(
                    h,
                    a.ptr(),
                    a.len(),
                    n,
                    opts,
                    o
                ));
            }
            7 => blob!("reverse", |o| tg_engine_reverse(h, lat, lon, n, o)),
            8 => {
                let m = coord(&mut c, 5_000.0);
                blob!("within_radius", |o| tg_engine_within_radius(
                    h, lat, lon, m, n, o
                ));
            }
            9 => blob!("admin_at", |o| tg_engine_admin_at(h, lat, lon, o)),
            10 => blob!("elevation_at", |o| tg_engine_elevation_at(h, lat, lon, o)),
            11 => blob!("postal_lookup", |o| tg_engine_postal_lookup(
                h,
                a.ptr(),
                a.len(),
                b.ptr(),
                b.len(),
                o
            )),
            12 => blob!("postal_prefix", |o| tg_engine_postal_prefix(
                h,
                a.ptr(),
                a.len(),
                b.ptr(),
                b.len(),
                n,
                o
            )),
            13 => blob!("postal_codes_of", |o| tg_engine_postal_codes_of(
                h,
                a.ptr(),
                a.len(),
                o
            )),
            14 => blob!("postal_at", |o| tg_engine_postal_at(h, lat, lon, o)),
            15 => blob!("tz_at", |o| tg_engine_tz_at(h, lat, lon, o)),
            16 => {
                let t = c.u64().cast_signed();
                blob!("tz_offset_at", |o| tg_engine_tz_offset_at(
                    h,
                    a.ptr(),
                    a.len(),
                    t,
                    o
                ));
            }
            17 => {
                let (y, mo, d) = (c.u32().cast_signed(), c.u32() % 16, c.u32() % 40);
                let (hh, mm, ss) = (c.u32() % 30, c.u32() % 70, c.u32() % 70);
                blob!("tz_local", |o| tg_engine_tz_local(
                    h,
                    a.ptr(),
                    a.len(),
                    y,
                    mo,
                    d,
                    hh,
                    mm,
                    ss,
                    o
                ));
            }
            18 => blob!("tz_zones", |o| tg_engine_tz_zones(h, o)),
            19 => blob!("tz_rules_start", |o| tg_engine_tz_rules_start(
                h,
                a.ptr(),
                a.len(),
                o
            )),
            20 => blob!("tz_version", |o| tg_engine_tz_version(h, o)),
            21 => blob!("by_iso", |o| tg_engine_by_iso(h, a.ptr(), a.len(), o)),
            22 => blob!("boundary", |o| tg_engine_boundary(
                h,
                a.ptr(),
                a.len(),
                n % 9,
                o
            )),
            23 => blob!("distance", |o| tg_engine_distance(
                h,
                a.ptr(),
                a.len(),
                b.ptr(),
                b.len(),
                o
            )),
            24 => {
                let mut count = 0_u32;
                // SAFETY: a writable out pointer.
                status("layers", unsafe { tg_engine_layers(h, &raw mut count) });
            }
            25 => {
                let mut gone = false;
                // SAFETY: a writable out pointer.
                status("remove", unsafe {
                    tg_engine_remove(h, n % 8, &raw mut gone)
                });
            }
            // A null handle is refused, never read.
            _ => blob!("get on null", |o| tg_engine_get(
                core::ptr::null(),
                a.ptr(),
                a.len(),
                o
            )),
        }
    }
    // SAFETY: the handle made above, freed once.
    unsafe { tg_engine_free(e) };
}
