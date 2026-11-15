//! The engine's fuzz harness (feature `fuzz`, D-331).
//!
//! [`engine`] takes a valid pack and fuzz bytes. One or two of the pack's sections are edited
//! and the pack is written again with every hash right
//! (`teigeodata_format::fuzz::edit_section`), so it passes `Layer::from_bytes` and the
//! section hashes, and every query reads the edited contents. The property is the one
//! `SECURITY.md` promises for a pack from an untrusted source: no panic, only a typed error or
//! an answer; and the edited sections' full check (`teigeodata_format::check`) never panics,
//! which reaches every kind's `open` and `check`. libFuzzer's `-rss_limit_mb` and `-timeout` bound memory and time. The same
//! function runs under `cargo test` on seeded inputs (`tests/fuzz.rs`) and under libFuzzer
//! (`fuzz/fuzz_targets/engine.rs`), as D-072 set for every harness.

#![allow(
    clippy::missing_panics_doc,
    clippy::panic,
    reason = "a test harness: a panic is how a property failure is reported"
)]

use std::vec::Vec;

use teigeodata_core::{E6, E7, IsoCode, PlaceId, Point};

use crate::{Engine, Layer, LayerOptions, Near, Outline, Place, Rect, Scratch, SearchOptions};

/// Fuzz bytes as values; runs out into zeros.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> u8 {
        let (v, rest) = self.0.split_first().map_or((0, &[][..]), |(v, r)| (*v, r));
        self.0 = rest;
        v
    }
    fn i32(&mut self) -> i32 {
        i32::from_le_bytes([0; 4].map(|_| self.u8()))
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes([0; 8].map(|_| self.u8()))
    }
    fn rest(&mut self) -> &'a [u8] {
        core::mem::take(&mut self.0)
    }
}

/// A point near `p` (within about 0.25°) or, when `far`, anywhere on the grid.
fn near(c: &mut Cursor<'_>, p: Point<E6>, far: bool) -> Point<E6> {
    let (lat, lon) = if far {
        (c.i32() % 90_000_001, c.i32() % 180_000_001)
    } else {
        (
            p.lat().saturating_add(c.i32() % 250_000),
            p.lon().saturating_add(c.i32() % 250_000),
        )
    };
    Point::from_units(
        lat.clamp(-90_000_000, 90_000_000),
        lon.clamp(-180_000_000, 180_000_000),
    )
    .unwrap_or(p)
}

/// Every point query at `at`. Errors are answers here: only a panic fails.
fn at_point(e: &Engine, at: Point<E6>, s: &mut Scratch, w: &mut Work) {
    let at7: Point<E7> = at.to_e7();
    let _ = e.reverse(at, 5, s, &mut w.near);
    let _ = e.within_radius(at, 5_000.0, 5, s, &mut w.near);
    let _ = e.mark_inside(at, s, &mut w.near);
    let _ = e.reverse_in_country(at, 5, s, &mut w.near);
    let rect = Rect {
        min_lat: at.lat().saturating_sub(50_000),
        max_lat: at.lat().saturating_add(50_000),
        min_lon: at.lon().saturating_sub(50_000),
        max_lon: at.lon().saturating_add(50_000),
    };
    let _ = e.within_bbox(rect, 16, s, &mut w.places);
    let _ = e.admin_at(at7, s, &mut w.places);
    let _ = e.countries_near(at7, 3_000, &mut w.places);
    let _ = e.missing_packs(at7, 3_000, &mut w.places);
    let _ = e.tz_at(at7);
    let _ = e.tz_offset_at_point(at7, 1_763_164_800);
    let _ = e.elevation_at(at7, s);
    let _ = e.postal_at(at7, s, &mut w.codes);
}

/// Every place query on `p`.
fn of_place(e: &Engine, p: &Place, s: &mut Scratch, w: &mut Work) {
    let _ = e.get(p.id);
    let mut buf = [0_u8; 256];
    let _ = e.name(p, &mut buf);
    let _ = e.names(p, &mut buf, |_, _| {});
    let _ = e.parent(p);
    let _ = e.ancestors(p, &mut w.places);
    let _ = e.children(p, &mut w.places);
    let _ = e.nearby(p, 5, s, &mut w.near);
    let _ = e.elevation(p);
    let _ = e.elevation_source(p);
    let _ = e.population(p);
    let _ = e.tz_of(p);
    let _ = e.zone_name(p);
    let _ = e.iso_code(p);
    let _ = e.bbox_of(p);
    for lod in [0, 3] {
        let _ = e.boundary_vertices(p, lod);
        if e.boundary(p, lod, &mut w.outline).is_ok() {
            w.json.clear();
            w.outline.write_geojson(&mut w.json);
        }
    }
    if e.postal_codes_of(p, &mut w.codes).is_ok() {
        let codes = core::mem::take(&mut w.codes);
        for code in codes.iter().take(2) {
            let _ = e.postal_replacement(code, &mut w.codes);
            let _ = e.postal_lookup(code.code(), Some(code.scheme()), &mut w.codes);
        }
    }
}

/// Buffers reused across queries, as an app would.
#[derive(Default)]
struct Work {
    near: Vec<Near>,
    places: Vec<Place>,
    hits: Vec<crate::Hit>,
    codes: Vec<crate::PostalCode>,
    outline: Outline,
    json: Vec<u8>,
}

/// `seed`, a valid pack, with sections edited by `data`, opened and asked every query: none
/// panics. Places are taken from the pack's first rows and from what the queries return, so
/// edits are reached through the paths apps use.
pub fn engine(seed: &[u8], data: &[u8]) {
    let Some(edit) = teigeodata_format::fuzz::edit_section(seed, data) else {
        return;
    };
    let (bytes, rest) = (edit.pack, edit.rest);
    // The edited sections' full check (`tgd verify --deep`) never panics either; the others
    // are the seed's, whole (`tests/fuzz.rs` checks every seed).
    if let Ok(p) = teigeodata_format::Pack::open(&bytes) {
        let edited = &edit.edited;
        teigeodata_format::check::sections_where(
            &p,
            |k, part| edited.contains(&(k, part)),
            |_, _| {},
        );
    }
    let Ok(layer) = Layer::from_bytes(bytes, LayerOptions::default()) else {
        return;
    };
    let rows = u32::try_from(layer.len()).unwrap_or(u32::MAX);
    let mut e = Engine::new();
    let id = e.add(layer);
    let mut c = Cursor(rest);
    let mut s = Scratch::default();
    let mut w = Work::default();

    let mut places: Vec<Place> = Vec::new();
    for row in [0, 1, rows / 2, rows.saturating_sub(1), u32::from(c.u8())] {
        if let Ok(Some(p)) = e.at_row(id, row) {
            places.push(p);
        }
    }
    let _ = e.at_row(id, rows);
    let origin = places
        .first()
        .map(|p| p.point)
        .or_else(|| Point::from_units(0, 0))
        .unwrap_or_else(|| panic!("the grid has no origin"));

    for p in &places {
        of_place(&e, p, &mut s, &mut w);
        at_point(&e, p.point, &mut s, &mut w);
    }
    for far in [false, true] {
        let at = near(&mut c, origin, far);
        at_point(&e, at, &mut s, &mut w);
    }
    // What a point query found is asked about too: rows the edits may have renumbered.
    let _ = e.admin_at(origin.to_e7(), &mut s, &mut w.places);
    for p in core::mem::take(&mut w.places).iter().take(4) {
        of_place(&e, p, &mut s, &mut w);
    }
    if let Some(id) = PlaceId::from_raw(c.u64()) {
        let _ = e.get(id);
    }
    for code in [*b"NP", *b"IN", [c.u8(), c.u8()]] {
        if let Ok(code) = IsoCode::from_bytes(&code) {
            let _ = e.by_iso(code);
        }
    }
    if let Ok(zones) = e.tz_zones() {
        for zone in zones.iter().take(3) {
            let _ = e.tz_offset_at(zone, 0);
            let _ = e.tz_rules_start(zone);
        }
    }
    let _ = e.tz_version();

    // Text queries: the fuzz bytes left over, as a search and as postal input.
    let text = String::from_utf8_lossy(c.rest());
    let _ = e.search(&text, 10, &mut s, &mut w.hits);
    let opts = SearchOptions {
        focus: Some(crate::Focus::new(origin)),
        ..SearchOptions::default()
    };
    let _ = e.search_with(&text, 10, &opts, &mut s, &mut w.hits);
    for hit in core::mem::take(&mut w.hits).iter().take(2) {
        of_place(&e, &hit.place, &mut s, &mut w);
    }
    let _ = e.postal_lookup(&text, None, &mut w.codes);
    let _ = e.postal_prefix(&text, None, 8, &mut w.codes);
}
