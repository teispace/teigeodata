//! Fuzz properties of the section payloads and the core codecs, beside the pack-level ones in
//! the parent module (D-072). The libFuzzer targets in `fuzz/` and the seeded driver in
//! `tests/fuzz.rs` both call them.
//!
//! Every harness reads its first byte as a mode. Half the inputs are taken as raw payload bytes;
//! the other half build a valid payload from a model drawn from the input, then edit up to three
//! of its bytes. An unedited payload must open, pass its check and read back as the model; an
//! edited one must never panic, and whatever passes the check must hold the view's invariants.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::verbose_bit_mask,
    clippy::many_single_char_names,
    reason = "harnesses: one model builder per section, grid units below 2^31, mode bits read as masks"
)]

use core::ops::ControlFlow;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::arcs::{self, ArcColumns, Arcs, PINNED};
use crate::blobs::{self, Blobs};
use crate::dem::{
    self, DATUM_EGM2008, Dem, DemScratch, Family, Grid, GridInput, THRESHOLDS, TileOptions,
};
use crate::hierarchy::{self, Hierarchy, MAX_DEPTH};
use crate::idmap::{self, IdMap};
use crate::place_index::{self, MAX_NODE, MIN_NODE, Neighbour, PlaceIndex, Rect};
use crate::places::{self, PlaceColumns, Places};
use crate::poly_index::{self, PolyIndex, polygon_box};
use crate::polygons::{self, PolygonInput, Polygons, arc_ref};
use crate::postal::{self, Code, CodeKind, Postal, Rule};
use crate::redirects::{self, Fate, Redirects, Resolved, resolve};
use crate::region::{self, Region};
use crate::search::fuzzy::Fuzzy;
use crate::search::{self, Search};
use crate::{Encoding, Kind, Pack, Writer};
use teigeodata_core::bitpack::{self, BLOCK_LEN, Coding, Packed};
use teigeodata_core::bits::{self as bitmap, Bits};
use teigeodata_core::table::{self as frame, Codec, Table, TableError};
use teigeodata_core::{E6, E7, Namespace, PlaceId, PlaceKind, Point, SERIAL_MAX, hilbert};

use super::{Cursor, limit};

/// The most rows a model has: several 128-value blocks, a short last one.
const MAX_ROWS: usize = limit(600, 200);
/// The most rows whose accessors are read on a raw payload (plus the last few).
const READ_ROWS: usize = limit(512, 64);

/// Rows worth reading: the first [`READ_ROWS`], the last few, and two past the end.
fn rows(len: usize) -> impl Iterator<Item = usize> {
    let tail = len.saturating_sub(8).max(READ_ROWS.min(len));
    (0..READ_ROWS.min(len)).chain(tail..len + 2)
}

/// Up to three byte edits (and, rarely, a changed item count); whether anything changed.
fn edit(c: &mut Cursor<'_>, body: &mut [u8], items: &mut u64) -> bool {
    let mut changed = false;
    for _ in 0..c.u8() % 4 {
        if body.is_empty() {
            break;
        }
        let at = c.below(body.len());
        body[at] ^= c.u8().max(1);
        changed = true;
    }
    if c.u8().is_multiple_of(16) {
        *items = items.wrapping_add(if c.u8() & 1 == 0 { 1 } else { u64::MAX });
        changed = true;
    }
    changed
}

/// A pack from the writer holding these TABLE sections; `None` if the writer refuses them.
fn pack(sections: &[(Kind, u64, &[u8])]) -> Option<Vec<u8>> {
    let mut w = Writer::new("np", "places", "2026.11.0").ok()?;
    for &(kind, items, body) in sections {
        w.section_bytes(kind, Encoding::TABLE, items, body).ok()?;
    }
    let bodies: Vec<&[u8]> = sections.iter().map(|s| s.2).collect();
    w.layout(false).ok()?.assemble(&bodies).ok()
}

/// Opens a pack the writer produced; the reader must accept it.
fn open(bytes: &[u8]) -> Pack<'_> {
    Pack::open(bytes).unwrap_or_else(|e| panic!("a written pack is refused: {e}"))
}

fn section<'a>(p: &Pack<'a>, kind: Kind) -> crate::Section<'a> {
    p.section(kind)
        .unwrap_or_else(|| panic!("a written {kind} section is missing"))
}

/// A valid ID from the input: any namespace, small or large serials.
fn place_id(c: &mut Cursor<'_>) -> PlaceId {
    let ns = c.u16() % 677;
    let ns = if ns == 0 {
        Namespace::WORLD
    } else {
        let n = ns - 1;
        Namespace::from_country([b'a' + (n / 26) as u8, b'a' + (n % 26) as u8])
            .unwrap_or(Namespace::WORLD)
    };
    let serial = if c.u8() & 3 == 0 {
        1 + c.u64() % SERIAL_MAX
    } else {
        1 + u64::from(c.u16())
    };
    PlaceId::new(ns, serial).unwrap_or_else(|| panic!("serial {serial} is in range"))
}

/// Section payloads in a pack: open never panics; a written payload reads back unchanged;
/// whatever passes its check holds the view's invariants.
pub fn sections(data: &[u8]) {
    let mut c = Cursor(data);
    let mode = c.u8();
    let structured = mode & 0x80 != 0;
    match mode % 12 {
        11 => postal_case(&mut c, structured),
        10 => dem_case(&mut c, structured),
        8 => arcs_case(&mut c, structured),
        9 => polygons_case(&mut c, structured),
        0 => places_case(&mut c, structured),
        1 => hierarchy_case(&mut c, structured),
        2 => idmap_case(&mut c, structured),
        3 => redirects_case(&mut c, structured),
        4 => place_index_case(&mut c, structured),
        5 => region_case(&mut c, structured),
        6 => search_case(&mut c, structured),
        _ => blobs_case(&mut c, structured),
    }
}

/// POSTAL with two schemes, digits (two lengths) and letters and digits with a separator,
/// codes from the input on up to 64 places. Lookups, prefixes, code text and place → codes
/// never panic and stay in range; a written section finds every code at its places.
fn postal_case(c: &mut Cursor<'_>, structured: bool) {
    let places = 1 + c.below(64) as u32;
    let rules = [
        Rule {
            name: "aa".into(),
            ignore: " -".into(),
            templates: vec!["999".into(), "99999".into()],
            ..Rule::default()
        },
        Rule {
            name: "b-1".into(),
            upper: true,
            ignore: " ".into(),
            prefixes: vec!["B-".into()],
            templates: vec!["A9 9A".into(), "A99 9A".into()],
            ..Rule::default()
        },
    ];
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let mut codes: Vec<Code> = Vec::new();
        let mut seen = BTreeSet::new();
        for _ in 0..c.below(48) {
            let digit = |c: &mut Cursor<'_>| char::from(b'0' + c.u8() % 4);
            let letter = |c: &mut Cursor<'_>| char::from(b'A' + c.u8() % 3);
            let (scheme, text) = if c.u8() & 1 == 0 {
                let n = if c.u8() & 1 == 0 { 3 } else { 5 };
                ("aa", (0..n).map(|_| digit(c)).collect::<String>())
            } else {
                let mut t = String::new();
                t.push(letter(c));
                t.push(digit(c));
                if c.u8() & 1 == 0 {
                    t.push(digit(c));
                }
                t.push(' ');
                t.push(digit(c));
                t.push(letter(c));
                ("b-1", t)
            };
            let place = c.below(places as usize) as u32;
            if !seen.insert((scheme, text.clone(), place)) {
                continue;
            }
            codes.push(Code {
                scheme: scheme.into(),
                code: text,
                place,
                kind: CodeKind::from_u32(u32::from(c.u8() % 4)).unwrap_or(CodeKind::Area),
                valid_from: None,
                valid_to: None,
                redirect: None,
            });
        }
        if codes.is_empty() {
            return;
        }
        body = postal::encode(&rules, &codes, places, c.u8() & 1 == 0)
            .unwrap_or_else(|e| panic!("valid postal codes are refused: {e}"));
        items = codes.len() as u64;
        if !edit(c, &mut body, &mut items) {
            model = Some(codes);
        }
    } else {
        items = u64::from(c.u8());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::POSTAL, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(post) = Postal::open(&section(&p, Kind::POSTAL), places as usize) else {
        assert!(model.is_none(), "a written POSTAL payload does not open");
        return;
    };
    let checked = post.check().is_ok();
    let mut buf = [0_u8; search::fst::MAX_KEY];
    for probe in [
        "0", "12", "012", "01230", "A1 2B", "a12 3c", "B-A1 2B", "A", "",
    ] {
        post.lookup(probe, None, |o| {
            assert!(o < post.keys(), "a lookup past the keys");
            ControlFlow::Continue(())
        });
        let mut n = 0;
        post.prefix(probe, None, |o| {
            assert!(o < post.keys(), "a prefix past the keys");
            n += 1;
            if n > 64 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
    }
    for place in 0..places.min(8) {
        post.records_at(place, |r| {
            assert!(r < post.records(), "a place index entry past the records");
            ControlFlow::Continue(())
        });
    }
    if checked {
        for o in 0..post.keys() {
            assert!(
                post.code(o, &mut buf).is_some(),
                "a checked key has no text"
            );
            for r in post.records_of(o) {
                let rec = post.record(r);
                assert!(rec.is_some_and(|x| x.place < places), "a checked record");
            }
        }
    }
    if let Some(codes) = &model {
        assert!(checked, "a written POSTAL payload fails its check");
        for code in codes {
            let mut hit = false;
            post.lookup(&code.code, Some(&code.scheme), |o| {
                hit |= post.records_of(o).any(|r| {
                    post.record(r)
                        .is_some_and(|x| x.place == code.place && x.kind == code.kind)
                });
                ControlFlow::Continue(())
            });
            assert!(hit, "a written code is not found at its place");
        }
    }
}

/// A DEM grid of 16 × 16 tiles somewhere on the globe: terrain as a random walk from the input
/// (small steps mostly, sometimes a cliff that escapes), some tiles absent. Point queries over
/// and around the grid never panic; a checked section decodes every present tile; a written one
/// decodes to the model.
fn dem_case(c: &mut Cursor<'_>, structured: bool) {
    const LOG2: u32 = 4;
    const SIDE: usize = 1 << LOG2;
    let (rows, cols) = (1 + c.below(2), 1 + c.below(3));
    let lon_step = 10 * (1 + u32::from(c.u8() % 3));
    // Inside the globe: 648,000 rows of 1″ from 90° N, 12,960,000 / step columns.
    let tile_cols_max = 12_960_000 / lon_step / SIDE as u32 - cols as u32;
    let grid = Grid {
        tile_row0: u32::from(c.u16()) % 40_000,
        tile_col0: u32::from(c.u16()) % tile_cols_max,
        tile_rows: rows as u32,
        tile_cols: cols as u32,
        lat_step: 10,
        lon_step,
        step_mm: 2_000,
        datum: DATUM_EGM2008,
        tile_log2: LOG2,
        first_tile: 0,
    };
    let (mut body, mut model) = (Vec::new(), None);
    let mut items = (rows * cols) as u64;
    if structured {
        let family = Family::standard();
        let opts = TileOptions { refine: false };
        let tiles: Vec<Option<Vec<i32>>> = (0..rows * cols)
            .map(|_| {
                if c.u8().is_multiple_of(5) {
                    return None;
                }
                let mut v = i32::from(c.u16()) - 500;
                Some(
                    (0..SIDE * SIDE)
                        .map(|_| {
                            let b = c.u8();
                            v += match b % 16 {
                                0 => i32::from(c.u16()) - 32_768,
                                k => i32::from(k % 5) - 2,
                            };
                            v
                        })
                        .collect(),
                )
            })
            .collect();
        let codes: Vec<Option<Vec<u8>>> = tiles
            .iter()
            .map(|t| {
                t.as_ref().map(|q| {
                    dem::encode_tile(q, LOG2, &family, &THRESHOLDS, &opts)
                        .unwrap_or_else(|e| panic!("a valid DEM tile is refused: {e}"))
                })
            })
            .collect();
        let refs: Vec<Option<&[u8]>> = codes.iter().map(Option::as_deref).collect();
        body = dem::encode(&[GridInput { grid, tiles: &refs }], &family, &THRESHOLDS)
            .unwrap_or_else(|e| panic!("a valid DEM grid is refused: {e}"));
        if !edit(c, &mut body, &mut items) {
            model = Some(tiles);
        }
    } else {
        items = u64::from(c.u8());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::DEM, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(d) = Dem::open(&section(&p, Kind::DEM)) else {
        assert!(model.is_none(), "a written DEM payload does not open");
        return;
    };
    let checked = d.check().is_ok();
    let mut s = DemScratch::new();
    // Queries over the grid and a margin around it, in 1e-7°: rows from 90° N, 0.1″ = 1e4 / 36.
    let (r0, c0) = (
        i64::from(grid.tile_row0) * SIDE as i64 - 3,
        i64::from(grid.tile_col0) * SIDE as i64 - 3,
    );
    for _ in 0..limit(64, 8) {
        let r = r0 + c.below(rows * SIDE + 6) as i64;
        let col = c0 + c.below(cols * SIDE + 6) as i64;
        let lat = 900_000_000 - (r * 10 * 10_000 + i64::from(c.u8()) * 100) / 36;
        let lon = -1_800_000_000
            + (col * i64::from(grid.lon_step) * 10_000 + i64::from(c.u8()) * 100) / 36;
        if let Some(at) = Point::<E7>::from_units(lat.max(-900_000_000) as i32, lon as i32)
            && let Some(h) = d.elevation(at, &mut s)
        {
            assert!(h.is_finite(), "a height that is not finite");
        }
    }
    if checked {
        for t in 0..d.tiles() {
            if let Some(q) = d.decode_tile(t, &mut s) {
                assert_eq!(q.len(), SIDE * SIDE, "a decoded tile of the wrong size");
            }
        }
    }
    if let Some(tiles) = &model {
        assert!(checked, "a written DEM payload fails its check");
        assert_eq!(d.grid(0), Some(grid));
        for (t, want) in tiles.iter().enumerate() {
            assert_eq!(
                d.decode_tile(t, &mut s).map(<[i32]>::to_vec).as_ref(),
                want.as_ref(),
                "tile {t}"
            );
        }
    }
}

/// A point from the input: in a small area mostly, sometimes anywhere, sometimes on a pole or
/// the antimeridian.
fn point(c: &mut Cursor<'_>) -> Point<E6> {
    let (lat, lon) = match c.u8() & 7 {
        0 => (
            (c.u32() % 180_000_001) as i32 - 90_000_000,
            (c.u32() % 360_000_001) as i32 - 180_000_000,
        ),
        1 => (90_000_000 * if c.u8() & 1 == 0 { 1 } else { -1 }, 0),
        2 => (
            i32::from(c.u16()),
            180_000_000 * if c.u8() & 1 == 0 { 1 } else { -1 },
        ),
        _ => (
            27_000_000 + i32::from(c.u16()),
            85_000_000 + i32::from(c.u16()),
        ),
    };
    Point::from_units(lat, lon).unwrap_or_else(|| panic!("a point on the grid is refused"))
}

/// `PLACE_INDEX` over a valid PLACES section: built and maybe edited, or raw bytes. Queries never
/// panic; on an index that passes its check, box queries equal brute force and the nearest row
/// is at the least distance.
#[allow(
    clippy::many_single_char_names,
    reason = "the query names of the brute-force model"
)]
/// A vertex for a region model: in a small box (deep leaves), anywhere, on the domain's edges,
/// or a vertex already drawn (shared borders and junctions). A `local` model stays in the box,
/// so its extent gets a fine grid.
fn region_vertex(c: &mut Cursor<'_>, seen: &[[i32; 2]], local: bool) -> [i32; 2] {
    let mode = c.u8() & 7;
    match if local && (2..=4).contains(&mode) {
        0
    } else {
        mode
    } {
        0 | 1 => [
            850_000_000 + (c.u32() % 2_000_001) as i32,
            270_000_000 + (c.u32() % 2_000_001) as i32,
        ],
        2 => [
            (i64::from(c.u32() % 3_600_000_001) - 1_800_000_000) as i32,
            (c.u32() % 1_800_000_001) as i32 - 900_000_000,
        ],
        3 => [
            [-1_800_000_000, 1_800_000_000][c.below(2)],
            (c.u32() % 1_800_000_001) as i32 - 900_000_000,
        ],
        4 => [
            (i64::from(c.u32() % 3_600_000_001) - 1_800_000_000) as i32,
            [-900_000_000, 900_000_000][c.below(2)],
        ],
        _ => seen.get(c.below(seen.len())).copied().unwrap_or([0, 0]),
    }
}

/// A query point: anywhere, or on, beside or between model vertices.
fn region_point(c: &mut Cursor<'_>, seen: &[[i32; 2]]) -> Point<E7> {
    let mut v = region_vertex(c, seen, false);
    if c.u8() & 1 == 1
        && let Some(w) = seen.get(c.below(seen.len()))
    {
        v = [i32::midpoint(v[0], w[0]), i32::midpoint(v[1], w[1])];
    }
    let d = i32::from(c.u8() % 3) - 1;
    Point::from_units(
        (v[1] + d).clamp(-900_000_000, 900_000_000),
        (v[0] - d).clamp(-1_800_000_000, 1_800_000_000),
    )
    .unwrap_or_else(|| Point::from_units(0, 0).unwrap_or_else(|| panic!("the origin")))
}

fn region_case(c: &mut Cursor<'_>, structured: bool) {
    // A written index has at least 4^MIN_TOP top cells: too slow to build under Miri, and
    // half the inputs natively (libFuzzer covers volume).
    let structured = structured && !cfg!(miri) && c.u8() & 1 == 0;
    let mut seen: Vec<[i32; 2]> = Vec::new();
    let local = c.u8() & 1 == 1;
    let regions: Vec<Vec<Vec<[i32; 2]>>> = (0..c.below(7))
        .map(|_| {
            (0..=c.below(2))
                .map(|_| {
                    (0..3 + c.below(limit(10, 4)))
                        .map(|_| {
                            let v = region_vertex(c, &seen, local);
                            seen.push(v);
                            v
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    let mut items = regions.len() as u64;
    let (mut body, mut valid) = (Vec::new(), false);
    if structured {
        let opts = region::Options {
            leaf_max: 1 + c.below(8),
            top: region::MIN_TOP,
            // Off, coarse or fine (the window of a small model spans few cells).
            fine_cells: [0, 16, 4096][usize::from(c.u8() % 3)],
        };
        body = region::encode(&regions, &opts).unwrap_or_else(|e| panic!("a region index: {e}"));
        valid = !edit(c, &mut body, &mut items);
    } else {
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::REGION, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let ix = match Region::open(&section(&p, Kind::REGION)) {
        Ok(ix) => ix,
        Err(e) => {
            assert!(!valid, "a written region index is refused: {e}");
            return;
        }
    };
    let checked = ix.check().is_ok();
    assert!(checked || !valid, "a written region index fails its check");
    let mut out = [0_u32; 8];
    for _ in 0..8 {
        let at = region_point(c, &seen);
        let n = ix.regions_at(at, &mut out);
        let got = out.get(..n.min(out.len())).unwrap_or_default();
        assert!(got.windows(2).all(|w| w[0] < w[1]), "regions out of order");
        if checked {
            assert!(
                got.iter().all(|&z| (z as usize) < ix.regions()),
                "a region past the end"
            );
        }
        if valid {
            assert_eq!(got, &region::even_odd(&regions, at)[..], "at {at:?}");
        }
    }
}

/// Characters of the search model: ASCII, two-byte and three-byte UTF-8, so walks assemble
/// multi-byte labels and edits cut them.
const SEARCH_CHARS: [char; 8] = ['a', 'b', 'c', 'k', '\u{e9}', '\u{928}', '\u{947}', ' '];

/// A short text over [`SEARCH_CHARS`].
fn search_text(c: &mut Cursor<'_>) -> String {
    (0..=c.below(6))
        .map(|_| SEARCH_CHARS[c.below(SEARCH_CHARS.len())])
        .collect()
}

fn search_case(c: &mut Cursor<'_>, structured: bool) {
    let rows = 1 + c.below(limit(300, 24));
    let importance: Vec<u8> = (0..rows).map(|_| c.u8()).collect();
    let mut model: BTreeMap<Vec<u8>, BTreeSet<u32>> = BTreeMap::new();
    for _ in 0..c.below(limit(200, 16)) {
        let mut key = vec![1 + c.u8() % 3];
        key.extend_from_slice(search_text(c).as_bytes());
        let list = model.entry(key).or_default();
        for _ in 0..=c.below(4) {
            list.insert(c.below(rows) as u32);
        }
    }
    let mut items = rows as u64;
    let (mut body, mut valid) = (Vec::new(), false);
    if structured {
        body = search::encode(&model, &importance).unwrap_or_else(|e| panic!("a search: {e}"));
        valid = !edit(c, &mut body, &mut items);
    } else {
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::SEARCH, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let ix = match Search::open(&section(&p, Kind::SEARCH)) {
        Ok(ix) => ix,
        Err(e) => {
            assert!(!valid, "a written search section is refused: {e}");
            return;
        }
    };
    let checked = ix.check().is_ok();
    assert!(
        checked || !valid,
        "a written search section fails its check"
    );
    let keys: Vec<(&Vec<u8>, &BTreeSet<u32>)> = model.iter().collect();
    for _ in 0..4 {
        let tag = 1 + c.u8() % 3;
        let query = match keys.get(c.below(keys.len() + 2)) {
            Some((k, _)) if k[0] == tag && c.u8() & 1 == 0 => {
                String::from_utf8(k[1..].to_vec()).unwrap_or_default()
            }
            _ => search_text(c),
        };
        let found = ix.get(tag, &query);
        let range = ix.range(tag, &query);
        let edits = c.u8() % 3;
        let mut walked = Vec::new();
        if let Some(m) = Fuzzy::new(&query, edits) {
            ix.walk(tag, &m, None, 1 << 14, |o, d| {
                walked.push((o, d));
                ControlFlow::Continue(())
            });
        }
        let mut best = Vec::new();
        let floor = c.u8();
        let read = ix.best_in(
            range.clone(),
            || i32::from(floor),
            |o| {
                best.push(o);
                ControlFlow::Continue(())
            },
        );
        assert!(read <= range.len(), "best_in reads past its range");
        if checked {
            assert!(
                range.end <= ix.keys() && range.start <= range.end,
                "a bad range"
            );
            assert!(
                walked.windows(2).all(|w| w[0].0 < w[1].0),
                "a walk out of order"
            );
            assert!(walked.iter().all(|w| w.0 < ix.keys() && w.1 <= edits));
            for o in walked.iter().map(|w| w.0).chain(found) {
                let mut last: Option<(u8, u32)> = None;
                ix.rows_of(o, |r| {
                    let here = (ix.importance(r), r);
                    assert!((r as usize) < ix.rows(), "a row past the end");
                    if let Some(l) = last {
                        assert!(
                            here.0 < l.0 || (here.0 == l.0 && here.1 > l.1),
                            "list order"
                        );
                    }
                    last = Some(here);
                    ControlFlow::Continue(())
                });
                assert!(last.is_some(), "an empty list");
            }
            assert!(
                best.iter()
                    .all(|&o| range.contains(&o) && i32::from(ix.key_max(o)) > i32::from(floor))
            );
        }
        if valid {
            let mut tagged = vec![tag];
            tagged.extend_from_slice(query.as_bytes());
            let at = model.keys().position(|k| *k == tagged).map(|i| i as u32);
            assert_eq!(found, at, "get {query:?}");
            let lo = model.range(..tagged.clone()).count() as u32;
            let hi = lo
                + model
                    .range(tagged.clone()..)
                    .take_while(|(k, _)| k.starts_with(&tagged))
                    .count() as u32;
            assert_eq!(range, lo..hi, "range {query:?}");
            if let Some(m) = Fuzzy::new(&query, edits) {
                let want: Vec<(u32, u8)> = keys
                    .iter()
                    .enumerate()
                    .filter(|(_, (k, _))| k[0] == tag)
                    .filter_map(|(i, (k, _))| {
                        let text = core::str::from_utf8(&k[1..]).unwrap_or_default();
                        m.matches(text).map(|d| (i as u32, d))
                    })
                    .collect();
                assert_eq!(walked, want, "walk {query:?} with {edits} edits");
            }
            for (i, (_, list)) in keys.iter().enumerate() {
                let mut got = Vec::new();
                ix.rows_of(i as u32, |r| {
                    got.push(r);
                    ControlFlow::Continue(())
                });
                let mut want: Vec<u32> = list.iter().copied().collect();
                want.sort_by_key(|&r| (core::cmp::Reverse(importance[r as usize]), r));
                assert_eq!(got, want, "list {i}");
            }
        }
    }
}

fn place_index_case(c: &mut Cursor<'_>, structured: bool) {
    let n = c.below(MAX_ROWS + 1);
    let mut points: Vec<Point<E6>> = (0..n).map(|_| point(c)).collect();
    points.sort_by_key(|p| hilbert(*p));
    let kinds = vec![PlaceKind::Settlement; n];
    let cols = PlaceColumns {
        points: &points,
        kinds: &kinds,
        ..PlaceColumns::default()
    };
    let places_body = places::encode(&cols).unwrap_or_else(|e| panic!("valid places: {e}"));
    let mut items = n as u64;
    let (mut body, mut valid) = (Vec::new(), false);
    if structured {
        let node = MIN_NODE + c.below(MAX_NODE - MIN_NODE + 1);
        body = place_index::encode(&points, node).unwrap_or_else(|e| panic!("an index: {e}"));
        valid = !edit(c, &mut body, &mut items);
    } else {
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[
        (Kind::PLACES, n as u64, &places_body),
        (Kind::PLACE_INDEX, items, &body),
    ]) else {
        return;
    };
    let p = open(&file);
    let places = Places::open(&section(&p, Kind::PLACES)).unwrap_or_else(|e| panic!("{e}"));
    let ix = match PlaceIndex::open(&section(&p, Kind::PLACE_INDEX), places) {
        Ok(ix) => ix,
        Err(e) => {
            assert!(!valid, "a written place index is refused: {e}");
            return;
        }
    };
    let checked = ix.check().is_ok();
    assert!(checked || !valid, "a written place index fails its check");
    for _ in 0..4 {
        let (a, b) = (point(c), point(c));
        let q = Rect {
            min_lat: a.lat().min(b.lat()),
            max_lat: a.lat().max(b.lat()),
            min_lon: a.lon(),
            max_lon: b.lon(),
        };
        let mut got = Vec::new();
        let _ = ix.within(q, |row| {
            got.push(row);
            ControlFlow::Continue(())
        });
        assert!(got.iter().all(|&r| r < n), "a row past the end");
        let at = point(c);
        let mut out = [Neighbour::default(); 8];
        let k = 1 + c.below(out.len());
        let found = ix.nearest(at, out.get_mut(..k).unwrap_or_default());
        assert!(found <= k.min(n));
        if checked {
            got.sort_unstable();
            let inside = |p: &Point<E6>| {
                let lon = if q.min_lon <= q.max_lon {
                    q.min_lon <= p.lon() && p.lon() <= q.max_lon
                } else {
                    p.lon() >= q.min_lon || p.lon() <= q.max_lon
                };
                q.min_lat <= p.lat() && p.lat() <= q.max_lat && lon
            };
            let want: Vec<usize> = (0..n)
                .filter(|&r| points.get(r).is_some_and(inside))
                .collect();
            assert_eq!(got, want, "a box query differs from brute force");
            assert_eq!(found, k.min(n));
            let query = place_index::nearest::Query::new(at);
            let best = points
                .iter()
                .map(|p| query.haversine(p.lat(), p.lon()))
                .fold(f64::INFINITY, f64::min);
            if let Some(first) = out.first().filter(|_| found > 0) {
                assert!(
                    first.haversine.to_bits() == best.to_bits(),
                    "not the nearest row"
                );
            }
        }
    }
}

/// BLOBS: zstd frames from `ruzstd`'s encoder, or raw bytes; every read is bounded, never
/// panics, and a written section reads back unchanged.
fn blobs_case(c: &mut Cursor<'_>, structured: bool) {
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let n = c.below(65);
        // Short runs of a few byte values, so frames compress; some blobs empty.
        let all: Vec<Vec<u8>> = (0..n)
            .map(|_| {
                let (len, a, b) = (c.below(limit(300, 40)), c.u8(), c.u8());
                (0..len).map(|i| if i % 7 < 4 { a } else { b }).collect()
            })
            .collect();
        let refs: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
        let target = 1 + c.below(limit(4096, 512));
        let fast = |raw: &[u8]| {
            Some(ruzstd::encoding::compress_to_vec(
                raw,
                ruzstd::encoding::CompressionLevel::Fastest,
            ))
        };
        body = blobs::encode(&refs, None, target, fast)
            .unwrap_or_else(|e| panic!("valid blobs are refused: {e}"));
        items = n as u64;
        if !edit(c, &mut body, &mut items) {
            model = Some(all);
        }
    } else {
        items = u64::from(c.u8());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::BLOBS, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(b) = Blobs::open(&section(&p, Kind::BLOBS)) else {
        assert!(model.is_none(), "a written BLOBS payload does not open");
        return;
    };
    let checked = b.check().is_ok();
    let Ok(mut d) = b.decoder() else {
        assert!(model.is_none(), "a written BLOBS dictionary does not parse");
        return;
    };
    let decoded = d.check().is_ok();
    for i in rows(b.len()) {
        if let Ok(blob) = d.get(i) {
            assert!(
                blob.len() <= blobs::MAX_FRAME,
                "a blob over the frame limit"
            );
        }
    }
    if let Some(all) = &model {
        assert!(
            checked && decoded,
            "a written BLOBS payload fails its check"
        );
        assert_eq!(b.len(), all.len());
        for (i, want) in all.iter().enumerate() {
            assert_eq!(d.get(i).ok(), Some(want.as_slice()), "blob {i}");
        }
    }
}

/// A model's rows, in Hilbert order: point, kind, admin level, class, flags, elevation.
type PlaceRow = (Point<E6>, PlaceKind, u8, u32, u32, Option<i16>);

fn places_case(c: &mut Cursor<'_>, structured: bool) {
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let n = c.below(MAX_ROWS + 1);
        let optional = c.u8();
        let mut rows: Vec<PlaceRow> = (0..n)
            .map(|_| {
                // A small area mostly, so the columns pack narrow; sometimes the whole grid.
                let wide = c.u8() & 7 == 0;
                let (lat, lon) = if wide {
                    (c.u32() % 180_000_001, c.u32() % 360_000_001)
                } else {
                    (
                        117_000_000 + u32::from(c.u16()),
                        264_000_000 + u32::from(c.u16()),
                    )
                };
                let p = Point::<E6>::from_units(lat as i32 - 90_000_000, lon as i32 - 180_000_000)
                    .unwrap_or_else(|| panic!("a point on the grid is refused"));
                let kind = PlaceKind::from_code(1 + u32::from(c.u8() % 7))
                    .unwrap_or(PlaceKind::Settlement);
                let level = if kind == PlaceKind::Admin {
                    c.u8() % 12
                } else {
                    0
                };
                // Heights across the whole i16 range sometimes; unknown sometimes.
                let elevation = match c.u8() % 4 {
                    0 => None,
                    1 => Some(c.u16().cast_signed().max(i16::MIN + 1)),
                    _ => Some(i16::from(c.u8()) * 8 - 200),
                };
                (
                    p,
                    kind,
                    level,
                    u32::from(c.u8() % 5),
                    u32::from(kind == PlaceKind::Admin && c.u8() & 1 == 1),
                    elevation,
                )
            })
            .collect();
        rows.sort_by_key(|r| hilbert(r.0));
        let points: Vec<_> = rows.iter().map(|r| r.0).collect();
        let kinds: Vec<_> = rows.iter().map(|r| r.1).collect();
        let on = |bit: u8| optional & bit != 0;
        let levels: Vec<u8> = rows.iter().filter(|_| on(1)).map(|r| r.2).collect();
        let classes: Vec<u32> = rows.iter().filter(|_| on(2)).map(|r| r.3).collect();
        let flags: Vec<u32> = rows.iter().filter(|_| on(4)).map(|r| r.4).collect();
        let elevations: Vec<Option<i16>> = rows.iter().filter(|_| on(8)).map(|r| r.5).collect();
        let cols = PlaceColumns {
            points: &points,
            kinds: &kinds,
            admin_levels: &levels,
            classes: &classes,
            flags: &flags,
            elevations: &elevations,
        };
        body = places::encode(&cols).unwrap_or_else(|e| panic!("a valid model is refused: {e}"));
        items = n as u64;
        // What a reader sees: an omitted optional column reads as 0.
        let seen: Vec<PlaceRow> = rows
            .iter()
            .map(|r| {
                let or0 = |bit: u8, v: u32| if optional & bit != 0 { v } else { 0 };
                (
                    r.0,
                    r.1,
                    or0(1, u32::from(r.2)) as u8,
                    or0(2, r.3),
                    or0(4, r.4),
                    r.5.filter(|_| optional & 8 != 0),
                )
            })
            .collect();
        if !edit(c, &mut body, &mut items) {
            model = Some(seen);
        }
    } else {
        items = u64::from(c.u16());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::PLACES, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(v) = Places::open(&section(&p, Kind::PLACES)) else {
        assert!(model.is_none(), "a written PLACES payload does not open");
        return;
    };
    let read = |r| {
        (
            v.point(r),
            v.kind(r),
            v.admin_level(r),
            v.class(r),
            v.flags(r),
            v.elevation(r),
        )
    };
    for r in rows(v.len()) {
        let _ = read(r);
    }
    let checked = v.check().is_ok();
    if let Some(m) = &model {
        assert!(checked, "a written PLACES payload fails its check");
        assert_eq!(v.len(), m.len());
        for (r, &(pt, kind, level, class, flags, elevation)) in m.iter().enumerate() {
            let want = (
                Some(pt),
                Some(kind),
                Some(level),
                Some(class),
                Some(flags),
                elevation,
            );
            assert_eq!(read(r), want, "PLACES row {r}");
        }
    }
    if checked {
        let mut last = 0;
        for r in 0..v.len().min(READ_ROWS) {
            let pt = v
                .point(r)
                .unwrap_or_else(|| panic!("a checked point is off the grid"));
            assert!(hilbert(pt) >= last, "checked rows out of Hilbert order");
            last = hilbert(pt);
            assert!(v.kind(r).is_some(), "a checked kind is unknown");
        }
        assert_eq!(read(v.len()), (None, None, None, None, None, None));
    }
}

fn hierarchy_case(c: &mut Cursor<'_>, structured: bool) {
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let n = c.below(MAX_ROWS + 1);
        let parents: Vec<Option<u32>> = (0..n)
            .map(|row| match c.u8() % 8 {
                // A root.
                0 | 1 => None,
                // Any row: may make a cycle, which the writer refuses.
                2 => Some(c.below(n) as u32),
                // An earlier row: always a forest, though maybe too deep.
                _ => (row > 0).then(|| c.below(row) as u32),
            })
            .collect();
        let Ok(b) = hierarchy::encode(&parents) else {
            return;
        };
        body = b;
        items = n as u64;
        if !edit(c, &mut body, &mut items) {
            model = Some(parents);
        }
    } else {
        items = u64::from(c.u16());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::HIERARCHY, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(h) = Hierarchy::open(&section(&p, Kind::HIERARCHY)) else {
        assert!(model.is_none(), "a written HIERARCHY payload does not open");
        return;
    };
    for r in rows(h.len()) {
        let _ = (h.parent(r), h.children(r).count(), h.ancestors(r).count());
    }
    let checked = h.check().is_ok();
    if let Some(m) = &model {
        assert!(checked, "a written HIERARCHY payload fails its check");
        assert_eq!(h.len(), m.len());
        for (r, &want) in m.iter().enumerate() {
            assert_eq!(h.parent(r), want.map(|p| p as usize), "parent of row {r}");
        }
    }
    if checked {
        let n = h.len();
        for r in rows(n).filter(|&r| r < n) {
            if let Some(up) = h.parent(r) {
                assert!(up < n && up != r, "a checked parent is out of range");
                assert!(
                    h.children(up).any(|k| k as usize == r),
                    "row {r} not listed"
                );
            }
            let mut prev = None;
            for k in h.children(r) {
                assert!(prev < Some(k), "children not ascending");
                prev = Some(k);
                assert_eq!(
                    h.parent(k as usize),
                    Some(r),
                    "a child names another parent"
                );
            }
            let chain: Vec<usize> = h.ancestors(r).collect();
            assert!(chain.len() <= MAX_DEPTH);
            if let Some(&top) = chain.last() {
                assert_eq!(
                    h.parent(top),
                    None,
                    "a checked chain is deeper than MAX_DEPTH"
                );
            }
        }
        assert_eq!(h.parent(n), None);
        assert_eq!(h.children(n).count(), 0);
    }
}

/// Distinct valid IDs from the input; `None` if two came out equal.
fn ids(c: &mut Cursor<'_>, n: usize) -> Option<Vec<PlaceId>> {
    let v: Vec<PlaceId> = (0..n).map(|_| place_id(c)).collect();
    let mut sorted = v.clone();
    sorted.sort_unstable();
    sorted.dedup();
    (sorted.len() == v.len()).then_some(v)
}

fn idmap_case(c: &mut Cursor<'_>, structured: bool) {
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let n = c.below(MAX_ROWS + 1);
        let Some(v) = ids(c, n) else { return };
        body = idmap::encode(&v).unwrap_or_else(|e| panic!("distinct IDs are refused: {e}"));
        items = n as u64;
        if !edit(c, &mut body, &mut items) {
            model = Some(v);
        }
    } else {
        items = u64::from(c.u16());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::IDMAP, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(m) = IdMap::open(&section(&p, Kind::IDMAP)) else {
        assert!(model.is_none(), "a written IDMAP payload does not open");
        return;
    };
    for r in rows(m.len()) {
        if let Some(id) = m.id(r) {
            let _ = m.row(id);
        }
    }
    let checked = m.check().is_ok();
    if let Some(v) = &model {
        assert!(checked, "a written IDMAP payload fails its check");
        assert_eq!(m.len(), v.len());
        for (r, &id) in v.iter().enumerate() {
            assert_eq!((m.id(r), m.row(id)), (Some(id), Some(r)), "IDMAP row {r}");
        }
    }
    if checked {
        for r in rows(m.len()).filter(|&r| r < m.len()) {
            let id = m.id(r).unwrap_or_else(|| panic!("a checked row has no ID"));
            assert_eq!(m.row(id), Some(r), "a checked ID does not find its row");
        }
        assert_eq!(m.id(m.len()), None);
    }
}

fn redirects_case(c: &mut Cursor<'_>, structured: bool) {
    // The current IDs are always valid; the redirects are built or raw.
    let n = c.below(limit(200, 40));
    let Some(current) = ids(c, n) else { return };
    let map_body = idmap::encode(&current).unwrap_or_else(|e| panic!("{e}"));
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let k = c.below(MAX_ROWS + 1);
        let Some(old) = ids(c, k) else { return };
        if old.iter().any(|o| current.contains(o)) {
            return;
        }
        let pairs: Vec<(PlaceId, Option<PlaceId>)> = old
            .into_iter()
            .map(|o| (o, (c.u8() & 3 != 0 && n > 0).then(|| current[c.below(n)])))
            .collect();
        body = redirects::encode(&pairs).unwrap_or_else(|e| panic!("valid pairs: {e}"));
        items = k as u64;
        if !edit(c, &mut body, &mut items) {
            model = Some(pairs);
        }
    } else {
        items = u64::from(c.u16());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[
        (Kind::IDMAP, n as u64, &map_body),
        (Kind::REDIRECTS, items, &body),
    ]) else {
        return;
    };
    let p = open(&file);
    let map = IdMap::open(&section(&p, Kind::IDMAP))
        .unwrap_or_else(|e| panic!("a written IDMAP payload does not open: {e}"));
    let Ok(red) = Redirects::open(&section(&p, Kind::REDIRECTS)) else {
        assert!(model.is_none(), "a written REDIRECTS payload does not open");
        return;
    };
    for &id in &current {
        let _ = (red.get(id), resolve(&map, Some(&red), id));
    }
    let checked = red.check(&map).is_ok();
    if let Some(pairs) = &model {
        assert!(checked, "a written REDIRECTS payload fails its check");
        assert_eq!(red.len(), pairs.len());
        for &(old, new) in pairs {
            let fate = new.map_or(Fate::Tombstone, Fate::Redirect);
            assert_eq!(red.get(old), Some(fate), "the fate of {old}");
            let want = match new {
                Some(new) => Resolved::Moved(new, map.row(new).unwrap_or(usize::MAX)),
                None => Resolved::Retired,
            };
            assert_eq!(resolve(&map, Some(&red), old), want, "resolving {old}");
        }
    }
    if checked {
        for (row, &id) in current.iter().enumerate() {
            assert_eq!(red.get(id), None, "a current ID is retired");
            assert_eq!(resolve(&map, Some(&red), id), Resolved::Row(row));
        }
    }
}

/// `Packed::parse` never panics; on an accepted column every accessor agrees with a full
/// decode; a written column reads back unchanged.
pub fn bitpack(data: &[u8]) {
    let mut c = Cursor(data);
    let mode = c.u8();
    let (mut bytes, mut model) = (Vec::new(), None);
    if mode & 0x80 != 0 {
        let n = usize::from(c.u16()) % limit(2000, 300);
        let mut acc = c.u32();
        let values: Vec<u32> = (0..n)
            .map(|_| match mode % 4 {
                0 => c.u32(),
                1 => u32::from(c.u8()),
                // Sorted, as IDs and running counts are.
                2 => {
                    acc = acc.wrapping_add(u32::from(c.u8() % 9));
                    acc
                }
                // Almost a ramp.
                _ => {
                    acc = acc.wrapping_add(1 + u32::from(c.u8() & 1));
                    acc
                }
            })
            .collect();
        bytes = match (mode >> 2) % 4 {
            0 => bitpack::encode(&values, Coding::For),
            1 => bitpack::encode(&values, Coding::Delta),
            2 => bitpack::encode(&values, Coding::Ramp),
            _ => bitpack::encode_auto(&values),
        };
        if !edit(&mut c, &mut bytes, &mut 0) {
            model = Some(values);
        }
    } else {
        bytes.extend_from_slice(c.rest());
    }
    let Ok(p) = Packed::parse(&bytes) else {
        assert!(model.is_none(), "a written column does not parse");
        return;
    };
    let n = p.len();
    assert_eq!(p.blocks(), n.div_ceil(BLOCK_LEN));
    let mut all = vec![0; p.blocks() * BLOCK_LEN];
    assert_eq!(p.decode_all(&mut all), n);
    all.truncate(n);
    if let Some(m) = &model {
        assert_eq!(&all, m, "a written column reads back changed");
    }
    for (i, &v) in all.iter().enumerate() {
        assert_eq!(p.get(i), Some(v), "get({i}) disagrees with decode_all");
    }
    assert_eq!(p.get(n), None);
    assert!(p.values(0..n).eq(all.iter().copied()));
    let mid = n / 3;
    assert!(p.values(mid..n + 5).eq(all[mid..].iter().copied()));
    let mut block = [0; BLOCK_LEN];
    for b in 0..p.blocks() {
        let start = b * BLOCK_LEN;
        let got = p.decode_block(b, &mut block);
        assert_eq!(&block[..got], &all[start..(start + BLOCK_LEN).min(n)]);
        assert_eq!(
            p.block_first(b),
            all.get(start).copied(),
            "block_first({b}) on a {:?} column",
            p.coding()
        );
        if let Some(v) = p.block_constant(b) {
            assert!(
                block[..got].iter().all(|&x| x == v),
                "block {b} is not constant"
            );
        }
    }
    assert_eq!(p.block_first(p.blocks()), None);
    assert_eq!(p.decode_block(p.blocks(), &mut block), 0);
}

/// `Table::parse` never panics; an accepted table re-encodes to its own bytes, and each column
/// opens exactly when its codec is the one asked for and that codec's parser accepts its bytes.
pub fn table(data: &[u8]) {
    let mut c = Cursor(data);
    let mode = c.u8();
    let mut bytes = Vec::new();
    let mut written = false;
    if mode & 0x80 != 0 {
        let mut id = u16::from(c.u8());
        let mut cols = Vec::new();
        for _ in 0..c.u8() % 6 {
            let n = c.below(limit(300, 40));
            if c.u8() & 1 == 0 {
                let values: Vec<u32> = (0..n).map(|_| u32::from(c.u8())).collect();
                cols.push((id, Codec::Bitpack, bitpack::encode_auto(&values)));
            } else {
                let ones: Vec<usize> = (0..n).filter(|_| c.u8() & 1 == 1).collect();
                let b = bitmap::encode(n, ones).unwrap_or_else(|e| panic!("{e}"));
                cols.push((id, Codec::Bits, b));
            }
            id = id.saturating_add(1 + u16::from(c.u8() % 4));
        }
        cols.dedup_by_key(|col| col.0);
        let refs: Vec<(u16, Codec, &[u8])> =
            cols.iter().map(|(i, k, b)| (*i, *k, &b[..])).collect();
        bytes =
            frame::encode_with(&refs).unwrap_or_else(|e| panic!("a valid table is refused: {e}"));
        written = !edit(&mut c, &mut bytes, &mut 0);
    } else {
        bytes.extend_from_slice(c.rest());
    }
    let Ok(t) = Table::parse(&bytes) else {
        assert!(!written, "a written table does not parse");
        return;
    };
    let ids: Vec<u16> = t.ids().collect();
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "column IDs not increasing"
    );
    let cols: Vec<(u16, Codec, &[u8])> = ids
        .iter()
        .map(|&id| {
            let (k, b) = t
                .column_raw(id)
                .unwrap_or_else(|| panic!("listed column {id} has no bytes"));
            assert_eq!(t.column_bytes(id), Some(b));
            (id, k, b)
        })
        .collect();
    let again = frame::encode_with(&cols).unwrap_or_else(|e| panic!("accepted columns: {e}"));
    assert_eq!(again, bytes, "an accepted table is not canonical");
    for &(id, k, b) in &cols {
        let packed = k == Codec::Bitpack && Packed::parse(b).is_ok();
        assert_eq!(t.column(id).is_ok(), packed);
        assert_eq!(t.optional(id).is_ok(), packed);
        assert_eq!(
            t.bits(id).is_ok(),
            k == Codec::Bits && Bits::parse(b).is_ok()
        );
        if written {
            assert!(
                packed || t.bits(id).is_ok(),
                "a written column {id} does not open"
            );
        }
    }
    if let Some(missing) = (0..=u16::MAX).find(|id| !ids.contains(id)) {
        assert_eq!(t.column_bytes(missing), None);
        assert!(matches!(t.column(missing), Err(TableError::Missing(m)) if m == missing));
        assert!(matches!(t.bits(missing), Err(TableError::Missing(m)) if m == missing));
        assert!(matches!(t.optional(missing), Ok(None)));
    }
}

/// `Bits::parse` and `check` never panic, nor do reads of a parsed bitmap; on a checked one,
/// ranks agree with the bits and it re-encodes to its own bytes; a written one reads back.
pub fn bits(data: &[u8]) {
    let mut c = Cursor(data);
    let mode = c.u8();
    let (mut bytes, mut model) = (Vec::new(), None);
    if mode & 0x80 != 0 {
        let len = usize::from(c.u16()) % limit(5000, 300);
        let mut set = vec![false; len];
        if len > 0 {
            for _ in 0..c.u8() {
                set[c.below(len)] = true;
            }
            if mode & 1 == 1 {
                // A dense run, so whole words and sub-blocks are full.
                let a = c.below(len);
                let b = (a + c.below(len)).min(len);
                set[a..b].fill(true);
            }
        }
        let ones = set.iter().enumerate().filter(|s| *s.1).map(|s| s.0);
        bytes = bitmap::encode(len, ones).unwrap_or_else(|e| panic!("a valid bitmap: {e}"));
        if !edit(&mut c, &mut bytes, &mut 0) {
            model = Some(set);
        }
    } else {
        bytes.extend_from_slice(c.rest());
    }
    let Ok(b) = Bits::parse(&bytes) else {
        assert!(model.is_none(), "a written bitmap does not parse");
        return;
    };
    let len = b.len();
    for i in rows(len) {
        let _ = (b.get(i), b.rank(i), b.index(i));
    }
    let _ = (b.count(), b.iter().take(READ_ROWS).count());
    let checked = b.check().is_ok();
    if let Some(m) = &model {
        assert!(checked, "a written bitmap fails its check");
        assert_eq!(len, m.len());
        assert!(
            (0..len).all(|i| b.get(i) == m[i]),
            "a written bitmap reads back changed"
        );
    }
    if checked {
        let mut rank = 0;
        for i in 0..len {
            assert_eq!(b.rank(i), rank, "rank({i})");
            assert_eq!(b.index(i), b.get(i).then_some(rank));
            rank += usize::from(b.get(i));
        }
        assert_eq!(
            (b.rank(len), b.rank(len + 9), b.count()),
            (rank, rank, rank)
        );
        assert!(!b.get(len));
        let ones: Vec<usize> = b.iter().collect();
        assert_eq!(ones.len(), rank);
        assert!(ones.iter().all(|&i| i < len && b.get(i)));
        let again = bitmap::encode(len, ones).unwrap_or_else(|e| panic!("checked bits: {e}"));
        assert_eq!(again, bytes, "a checked bitmap is not canonical");
    }
}

/// `ARCS_OVERVIEW` of `full` at a random level: written, it passes both checks and reads like
/// `full` at its level and coarser; edited, if both checks still pass, it still reads the same.
fn overview_checks(c: &mut Cursor<'_>, full: &Arcs<'_>) {
    if !full.has_levels() || full.check().is_err() {
        return;
    }
    let from = 1 + u8::try_from(c.below(6)).unwrap_or(0);
    let mut body =
        arcs::overview(full, from).unwrap_or_else(|e| panic!("a valid overview is refused: {e}"));
    let mut items = full.len() as u64;
    let edited = edit(c, &mut body, &mut items);
    let checked = Arcs::open(&crate::Section::unpacked(Kind::ARCS_OVERVIEW, items, &body))
        .and_then(|ov| {
            ov.check()
                .and_then(|()| ov.check_overview(full))
                .map(|()| ov)
        });
    let ov = match checked {
        Ok(ov) => ov,
        Err(e) => {
            assert!(edited, "a written overview fails its checks: {e}");
            return;
        }
    };
    assert_eq!(ov.overview_from(), Some(from));
    for a in 0..full.len() {
        for lod in from..PINNED {
            for reversed in [false, true] {
                assert!(
                    full.points(a, reversed, lod)
                        .eq(ov.points(a, reversed, lod)),
                    "arc {a} at level {lod} reads differently from its overview"
                );
            }
        }
    }
}

/// One model arc: a random walk from the input's seed, sometimes on a coarse grid (a scale),
/// sometimes closed, with levels when `levels` (ends and a closed arc's anchor pinned).
fn model_arc(c: &mut Cursor<'_>, levels: bool) -> (Vec<Point<E7>>, Vec<u8>) {
    let mut seed = c.u64() | 1;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let len = 2 + c.below(limit(300, 140));
    let unit: i64 = [1, 1, 10, 1000][c.u8() as usize % 4];
    let wide = c.u8() & 7 == 0;
    let unit = if wide { 1 } else { unit };
    let (mut x, mut y): (i64, i64) = if wide {
        (
            (rnd() % 3_400_000_000) as i64 - 1_700_000_000,
            (rnd() % 1_700_000_000) as i64 - 850_000_000,
        )
    } else {
        (850_000_000 + (rnd() % 100_000) as i64 * unit, 280_000_000)
    };
    let mut pts = Vec::with_capacity(len + 1);
    for _ in 0..len {
        pts.push(
            Point::<E7>::from_units(y as i32, x as i32)
                .unwrap_or_else(|| panic!("a model point is off the grid")),
        );
        let span = if wide { 1 << 20 } else { 1 << 12 };
        let (mut dx, dy) = (
            (rnd() % span) as i64 - span as i64 / 2,
            (rnd() % span) as i64 - span as i64 / 2,
        );
        if dx == 0 && dy == 0 {
            dx = 1;
        }
        x = (x + dx * unit).clamp(-1_800_000_000, 1_800_000_000);
        y = (y + dy * unit).clamp(-900_000_000, 900_000_000);
    }
    pts.dedup();
    if pts.len() < 2 {
        pts.push(Point::<E7>::from_units(pts[0].lat(), pts[0].lon() ^ 1).unwrap_or(pts[0]));
    }
    if c.u8() & 3 == 0 && pts.len() >= 3 && pts[pts.len() - 1] != pts[0] {
        pts.push(pts[0]);
    }
    let closed = pts.len() >= 4 && pts[0] == pts[pts.len() - 1];
    if !closed && pts[0] == pts[pts.len() - 1] {
        pts.pop();
    }
    let mut lv: Vec<u8> = if levels {
        (0..pts.len()).map(|_| (rnd() % 8) as u8).collect()
    } else {
        Vec::new()
    };
    if levels {
        let n = lv.len();
        lv[0] = PINNED;
        lv[n - 1] = PINNED;
        if closed {
            lv[n / 2] = PINNED;
        }
    }
    (pts, lv)
}

fn arcs_case(c: &mut Cursor<'_>, structured: bool) {
    let (mut body, mut model) = (Vec::new(), None);
    let mut items;
    if structured {
        let n = c.below(limit(10, 4) + 1);
        let levels = c.u8() & 1 == 0 && n > 0;
        let arcs: Vec<(Vec<Point<E7>>, Vec<u8>)> = (0..n).map(|_| model_arc(c, levels)).collect();
        let pts: Vec<&[Point<E7>]> = arcs.iter().map(|a| &a.0[..]).collect();
        let lvs: Vec<&[u8]> = if levels {
            arcs.iter().map(|a| &a.1[..]).collect()
        } else {
            Vec::new()
        };
        let tol = [100, 500, 2_000, 10_000, 50_000, 200_000];
        let cols = ArcColumns {
            arcs: &pts,
            levels: &lvs,
            tolerances_cm: if levels { &tol } else { &[] },
        };
        body = arcs::encode(&cols).unwrap_or_else(|e| panic!("a valid arc model is refused: {e}"));
        items = n as u64;
        if !edit(c, &mut body, &mut items) {
            model = Some(arcs);
        }
    } else {
        items = u64::from(c.u8());
        body.extend_from_slice(c.rest());
    }
    let Some(file) = pack(&[(Kind::ARCS, items, &body)]) else {
        return;
    };
    let p = open(&file);
    let Ok(v) = Arcs::open(&section(&p, Kind::ARCS)) else {
        assert!(model.is_none(), "a written ARCS payload does not open");
        return;
    };
    // Reads never panic, whatever the bytes.
    for a in rows(v.len()) {
        for lod in [0, 3, PINNED, 9] {
            let _ = v.points(a, false, lod).take(4 * BLOCK_LEN).count();
            let _ = v.points(a, true, lod).take(4 * BLOCK_LEN).count();
            let _ = (v.count(a, lod), v.tolerance_cm(lod), v.first(a));
        }
    }
    let checked = v.check().is_ok();
    if let Some(m) = &model {
        assert!(
            checked,
            "a written ARCS payload fails its check: {:?}",
            v.check()
        );
        assert_eq!(v.len(), m.len());
        for (a, (pts, lv)) in m.iter().enumerate() {
            for lod in 0..=PINNED {
                let want: Vec<Point<E7>> = pts
                    .iter()
                    .zip(0..)
                    .filter(|&(_, i)| lv.is_empty() || lv[i] >= lod)
                    .map(|(p, _)| *p)
                    .collect();
                let got: Vec<Point<E7>> = v.points(a, false, lod).collect();
                assert_eq!(got, want, "arc {a} at level {lod}");
                let mut back: Vec<Point<E7>> = v.points(a, true, lod).collect();
                back.reverse();
                assert_eq!(back, want, "arc {a} reversed at level {lod}");
                assert_eq!(v.count(a, lod), want.len());
            }
        }
    }
    if checked {
        // An oracle that shares nothing with the reader: each arc from its base and the raw
        // delta columns, without checkpoints.
        let t = Table::parse(section(&p, Kind::ARCS).bytes()).unwrap_or_else(|e| panic!("{e:?}"));
        let col = |id| {
            t.column(id)
                .unwrap_or_else(|e| panic!("column {id}: {e:?}"))
        };
        let (start, bx, by, dx, dy) = (
            col(arcs::START),
            col(arcs::BASE_X),
            col(arcs::BASE_Y),
            col(arcs::DX),
            col(arcs::DY),
        );
        let scale = t.optional(arcs::SCALE).ok().flatten();
        for a in rows(v.len()).filter(|&a| a < v.len()) {
            let (s, e) = (
                start.get(a).unwrap_or(0) as usize,
                start.get(a + 1).unwrap_or(0) as usize,
            );
            let mul = 10_i64.pow(scale.and_then(|c| c.get(a)).unwrap_or(0));
            let (mut x, mut y) = (
                i64::from(bitpack::unbias(bx.get(a).unwrap_or(0))),
                i64::from(bitpack::unbias(by.get(a).unwrap_or(0))),
            );
            let mut want = Vec::new();
            for i in s..e {
                if i > s {
                    x += i64::from(bitpack::unzigzag(dx.get(i).unwrap_or(0)).cast_signed()) * mul;
                    y += i64::from(bitpack::unzigzag(dy.get(i).unwrap_or(0)).cast_signed()) * mul;
                }
                want.push(
                    Point::<E7>::from_units(y as i32, x as i32)
                        .unwrap_or_else(|| panic!("a checked vertex is off the grid")),
                );
            }
            let got: Vec<Point<E7>> = v.points(a, false, 0).collect();
            assert_eq!(got, want, "a checked arc differs from its deltas");
        }
        for a in rows(v.len()).filter(|&a| a < v.len()) {
            let fwd: Vec<Point<E7>> = v.points(a, false, 0).collect();
            let mut back: Vec<Point<E7>> = v.points(a, true, 0).collect();
            back.reverse();
            assert_eq!(fwd, back, "a checked arc reads differently backwards");
            assert_eq!(
                Some(fwd.len()),
                v.range(a).map(|r| r.len()),
                "a checked arc loses vertices"
            );
            assert!(
                fwd.windows(2).all(|w| w[0] != w[1]),
                "a checked arc repeats a vertex"
            );
            assert_eq!(fwd.first().copied(), v.first(a));
        }
    }
}

/// A model topology: a grid of `w` × `h` square cells sharing their edges as arcs (with a few
/// bent vertices), a parent polygon over the whole grid, and in cell 0 a square hole filled by an
/// island polygon. Returns the arcs and, per polygon, its rings as lists of references.
struct Topo {
    arcs: Vec<Vec<Point<E7>>>,
    /// Per polygon; polygon 0 is the parent (group 0).
    polys: Vec<ModelPolygon>,
}

/// A model polygon: its row, its parts as rings of references, its label.
type ModelPolygon = (u32, Vec<Vec<Vec<u32>>>, Point<E7>);

fn grid_topo(c: &mut Cursor<'_>) -> Topo {
    let (w, h) = (1 + c.below(3), 1 + c.below(3));
    let size = 1_000 + i32::from(c.u16()) * 4;
    let (x0, y0) = (
        850_000_000 + i32::from(c.u16()) * 100,
        280_000_000 + i32::from(c.u16()) * 100,
    );
    let at =
        |x: i32, y: i32| Point::<E7>::from_units(y, x).unwrap_or_else(|| panic!("model point"));
    let node = |i: usize, j: usize| (x0 + i as i32 * size, y0 + j as i32 * size);
    let mut arcs: Vec<Vec<Point<E7>>> = Vec::new();
    // An edge from node a to node b, bent by up to a tenth of a cell sideways.
    let mut edge = |c: &mut Cursor<'_>, a: (i32, i32), b: (i32, i32)| {
        let mut pts = vec![at(a.0, a.1)];
        let bends = c.u8() % 4;
        for k in 1..=bends {
            let t = i32::from(k) * size / (i32::from(bends) + 1);
            let off = i32::from(c.u8()) % (size / 10) - size / 20;
            let p = if a.1 == b.1 {
                (a.0 + t * (b.0 - a.0).signum(), a.1 + off)
            } else {
                (a.0 + off, a.1 + t * (b.1 - a.1).signum())
            };
            pts.push(at(p.0, p.1));
        }
        pts.push(at(b.0, b.1));
        pts.dedup();
        arcs.push(pts);
        (arcs.len() - 1) as u32
    };
    // Horizontal edges hz[j][i] from node (i, j) to (i + 1, j); vertical vt[i][j] from (i, j) to (i, j + 1).
    let hz: Vec<Vec<u32>> = (0..=h)
        .map(|j| {
            (0..w)
                .map(|i| edge(c, node(i, j), node(i + 1, j)))
                .collect()
        })
        .collect();
    let vt: Vec<Vec<u32>> = (0..=w)
        .map(|i| {
            (0..h)
                .map(|j| edge(c, node(i, j), node(i, j + 1)))
                .collect()
        })
        .collect();
    // The hole and island: a closed square in the lower-left quarter of cell 0.
    let (q, (hx, hy)) = (size / 4, node(0, 0));
    let sq = [
        (hx + q / 2, hy + q / 2),
        (hx + q + q / 2, hy + q / 2),
        (hx + q + q / 2, hy + q + q / 2),
        (hx + q / 2, hy + q + q / 2),
        (hx + q / 2, hy + q / 2),
    ];
    arcs.push(sq.iter().map(|p| at(p.0, p.1)).collect());
    let island = (arcs.len() - 1) as u32;
    let mut polys = Vec::new();
    let mut outer = Vec::new();
    outer.extend((0..w).map(|i| arc_ref(hz[0][i], false)));
    outer.extend((0..h).map(|j| arc_ref(vt[w][j], false)));
    outer.extend((0..w).rev().map(|i| arc_ref(hz[h][i], true)));
    outer.extend((0..h).rev().map(|j| arc_ref(vt[0][j], true)));
    let centre = |i: usize, j: usize| {
        let (x, y) = node(i, j);
        at(x + size * 3 / 4, y + size * 3 / 4)
    };
    polys.push((0, vec![vec![outer]], centre(0, 0)));
    let mut row = 1;
    for j in 0..h {
        for i in 0..w {
            let ring = vec![
                arc_ref(hz[j][i], false),
                arc_ref(vt[i + 1][j], false),
                arc_ref(hz[j + 1][i], true),
                arc_ref(vt[i][j], true),
            ];
            let mut rings = vec![ring];
            if i == 0 && j == 0 {
                rings.push(vec![arc_ref(island, true)]);
            }
            polys.push((row, vec![rings], centre(i, j)));
            row += 1;
        }
    }
    polys.push((
        row,
        vec![vec![vec![arc_ref(island, false)]]],
        at(hx + q, hy + q),
    ));
    Topo { arcs, polys }
}

fn polygons_case(c: &mut Cursor<'_>, structured: bool) {
    let topo = grid_topo(c);
    let pts: Vec<&[Point<E7>]> = topo.arcs.iter().map(|a| &a[..]).collect();
    let arcs_body = arcs::encode(&ArcColumns {
        arcs: &pts,
        ..ArcColumns::default()
    })
    .unwrap_or_else(|e| panic!("model arcs: {e}"));
    // Two ring lists per polygon so PolygonInput can borrow them.
    let ring_refs: Vec<Vec<&[u32]>> = topo
        .polys
        .iter()
        .map(|p| p.1[0].iter().map(|r| &r[..]).collect())
        .collect();
    let parts: Vec<[&[&[u32]]; 1]> = ring_refs.iter().map(|r| [&r[..]]).collect();
    let input: Vec<PolygonInput<'_>> = topo
        .polys
        .iter()
        .zip(&parts)
        .map(|(p, parts)| PolygonInput {
            row: p.0,
            parts: &parts[..],
            label: p.2,
            area_ha: 1,
        })
        .collect();
    let groups = [0_u32, 1];
    let mut body = polygons::encode(&input, &groups)
        .unwrap_or_else(|e| panic!("a valid polygon model is refused: {e}"));
    let mut items = input.len() as u64;
    // POLY_INDEX over the model's boxes, computed here from the model arcs.
    let boxes: Vec<Rect> = topo
        .polys
        .iter()
        .map(|p| {
            let mut r = Rect::EMPTY;
            for &x in p.1[0].iter().flatten() {
                for v in &topo.arcs[(x >> 1) as usize] {
                    r.grow(&Rect::around_e7(v.lat(), v.lon()));
                }
            }
            r
        })
        .collect();
    let node = MIN_NODE + c.below(MAX_NODE - MIN_NODE + 1);
    let mut ix_body = poly_index::encode(&boxes, node)
        .unwrap_or_else(|e| panic!("a valid polygon index is refused: {e}"));
    let mut ix_items = boxes.len() as u64;
    // ADMIN_REGION on the model arcs, region i = polygon i.
    let model_arcs = Arcs::open(&crate::Section::unpacked(
        Kind::ARCS,
        topo.arcs.len() as u64,
        &arcs_body,
    ))
    .unwrap_or_else(|e| panic!("model arcs: {e}"));
    let arc_xy: Vec<Vec<[i32; 2]>> = topo
        .arcs
        .iter()
        .map(|a| a.iter().map(|p| [p.lon(), p.lat()]).collect())
        .collect();
    let scales: Vec<u32> = if model_arcs.has_scale() {
        (0..topo.arcs.len()).map(|a| model_arcs.scale(a)).collect()
    } else {
        Vec::new()
    };
    let uses: Vec<Vec<u32>> = topo
        .polys
        .iter()
        .map(|p| p.1[0].iter().flatten().map(|r| r >> 1).collect())
        .collect();
    let leaf_max = 1 + c.below(32);
    let mut rg_body = region::encode_on_arcs(
        &arc_xy,
        &scales,
        &uses,
        &region::Options {
            leaf_max,
            top: 8,
            fine_cells: [0, 16, 4096][usize::from(c.u8() % 3)],
        },
    )
    .unwrap_or_else(|e| panic!("a valid admin region is refused: {e}"));
    let mut rg_items = uses.len() as u64;
    let (mut model, mut ix_model, mut rg_model) = (true, true, true);
    match (structured, c.u8() % 4) {
        (true, 0) => ix_model = !edit(c, &mut ix_body, &mut ix_items),
        (true, 1) => rg_model = !edit(c, &mut rg_body, &mut rg_items),
        (true, _) => model = !edit(c, &mut body, &mut items),
        (false, 0) => {
            ix_items = u64::from(c.u8());
            ix_body = c.rest().to_vec();
            ix_model = false;
        }
        (false, 1) => {
            rg_items = u64::from(c.u8());
            rg_body = c.rest().to_vec();
            rg_model = false;
        }
        (false, _) => {
            items = u64::from(c.u8());
            body = c.rest().to_vec();
            model = false;
        }
    }
    let places = topo.polys.len() + 1;
    let Some(file) = pack(&[
        (Kind::ARCS, topo.arcs.len() as u64, &arcs_body),
        (Kind::POLYGONS, items, &body),
        (Kind::POLY_INDEX, ix_items, &ix_body),
        (Kind::ADMIN_REGION, rg_items, &rg_body),
    ]) else {
        return;
    };
    let p = open(&file);
    let a = Arcs::open(&section(&p, Kind::ARCS)).unwrap_or_else(|e| panic!("model arcs: {e}"));
    admin_region_checks(c, &a, &topo, rg_model, &p);
    overview_checks(c, &a);
    let Ok(v) = Polygons::open(&section(&p, Kind::POLYGONS)) else {
        assert!(!model, "a written POLYGONS payload does not open");
        return;
    };
    // Probes: random near the grid, or exactly on a vertex, or on a vertex's latitude, where
    // the border rule decides.
    let verts: Vec<Point<E7>> = topo.arcs.iter().flatten().copied().collect();
    let probe = |c: &mut Cursor<'_>| {
        let l = verts[c.below(verts.len())];
        let (dy, dx) = match c.u8() % 4 {
            0 => (0, 0),
            1 => (0, i32::from(c.u16()) - 32_768),
            _ => (
                i32::from(c.u16()) * 3 - 98_304,
                i32::from(c.u16()) * 3 - 98_304,
            ),
        };
        Point::<E7>::from_units(l.lat() + dy, l.lon() + dx).unwrap_or(l)
    };
    for q in rows(v.len()) {
        let _ = (
            v.row(q),
            v.label(q),
            v.area_ha(q),
            v.parts(q),
            v.polygon_of(q as u32),
        );
        let _ = v.contains(&a, q, probe(c));
        for r in 0..8 {
            let _ = v.ring(&a, r, 0).take(2000).count();
            let _ = v.refs(r).take(64).count();
        }
    }
    let checked = v.check(&a, places).is_ok();
    // The oracle: REGION's own even–odd over the decoded rings.
    let rings_of = |q: usize| -> Vec<Vec<[i32; 2]>> {
        let mut out = Vec::new();
        for part in v.parts(q).unwrap_or(0..0) {
            for r in v.rings(part).unwrap_or(0..0) {
                out.push(v.ring(&a, r, 0).map(|p| [p.lon(), p.lat()]).collect());
            }
        }
        out
    };
    if model {
        assert!(
            checked,
            "a written POLYGONS payload fails its check: {:?}",
            v.check(&a, places)
        );
        assert_eq!(v.len(), topo.polys.len());
        for (q, (row, rings, label)) in topo.polys.iter().enumerate() {
            assert_eq!(
                (v.row(q), v.label(q), v.polygon_of(*row)),
                (Some(*row), Some(*label), Some(q))
            );
            let decoded = rings_of(q);
            assert_eq!(decoded.len(), rings[0].len());
            for (d, refs) in decoded.iter().zip(&rings[0]) {
                // Built here from the model arcs, not by the reader.
                let mut want: Vec<[i32; 2]> = Vec::new();
                for &r in refs {
                    let mut arc: Vec<[i32; 2]> = topo.arcs[(r >> 1) as usize]
                        .iter()
                        .map(|p| [p.lon(), p.lat()])
                        .collect();
                    if r & 1 == 1 {
                        arc.reverse();
                    }
                    let skip = usize::from(!want.is_empty());
                    want.extend(arc.into_iter().skip(skip));
                }
                assert_eq!(d, &want, "polygon {q} ring");
            }
        }
    }
    if checked {
        let all: Vec<Vec<Vec<[i32; 2]>>> = (0..v.len()).map(rings_of).collect();
        for q in 0..v.len().min(64) {
            let label = v
                .label(q)
                .unwrap_or_else(|| panic!("a checked label is off the grid"));
            assert_eq!(
                region::even_odd(&all[q..=q], label),
                vec![0],
                "a checked label is outside polygon {q}"
            );
        }
        for _ in 0..16 {
            let at = probe(c);
            let want = region::even_odd(&all, at);
            let got: Vec<u32> = (0..v.len())
                .filter(|&q| v.contains(&a, q, at))
                .map(|q| q as u32)
                .collect();
            assert_eq!(got, want, "contains at {at}");
        }
    }
    poly_index_checks(c, &v, &a, &topo, checked, model && ix_model, &p, probe);
}

/// `ADMIN_REGION` over the polygons case: queries and the check never panic; a written index
/// checks and answers as the even–odd oracle over the model's own rings, on probes at vertices,
/// on vertex latitudes and near them. A corrupt index that still checks may answer wrongly
/// (the check does not re-derive sets), so only the model is held to the oracle.
fn admin_region_checks(c: &mut Cursor<'_>, a: &Arcs<'_>, topo: &Topo, model: bool, p: &Pack<'_>) {
    let Ok(r) = Region::open_on_arcs(&section(p, Kind::ADMIN_REGION), a) else {
        assert!(!model, "a written ADMIN_REGION payload does not open");
        return;
    };
    let verts: Vec<Point<E7>> = topo.arcs.iter().flatten().copied().collect();
    let probe = |c: &mut Cursor<'_>| {
        let l = verts[c.below(verts.len())];
        let (dy, dx) = match c.u8() % 3 {
            0 => (0, 0),
            1 => (0, i32::from(c.u16()) - 32_768),
            _ => (i32::from(c.u16()) - 32_768, i32::from(c.u16()) - 32_768),
        };
        Point::<E7>::from_units(l.lat() + dy, l.lon() + dx).unwrap_or(l)
    };
    let mut out = [0_u32; region::MAX_SET];
    for _ in 0..8 {
        let _ = r.regions_at(probe(c), &mut out);
    }
    let checked = r.check().is_ok();
    if !model {
        return;
    }
    assert!(
        checked,
        "a written ADMIN_REGION fails its check: {:?}",
        r.check()
    );
    let rings: Vec<Vec<Vec<[i32; 2]>>> = topo
        .polys
        .iter()
        .map(|poly| {
            poly.1[0]
                .iter()
                .map(|refs| {
                    let mut ring: Vec<[i32; 2]> = Vec::new();
                    for &x in refs {
                        let mut arc: Vec<[i32; 2]> = topo.arcs[(x >> 1) as usize]
                            .iter()
                            .map(|q| [q.lon(), q.lat()])
                            .collect();
                        if x & 1 == 1 {
                            arc.reverse();
                        }
                        let skip = usize::from(!ring.is_empty());
                        ring.extend(arc.into_iter().skip(skip));
                    }
                    ring
                })
                .collect()
        })
        .collect();
    for _ in 0..24 {
        let at = probe(c);
        let k = r.regions_at(at, &mut out);
        assert_eq!(
            out[..k.min(out.len())].to_vec(),
            region::even_odd(&rings, at),
            "ADMIN_REGION at {at}"
        );
    }
}

/// `POLY_INDEX` over the polygons case: queries never panic; a written index checks; on a
/// checked index, `containing` is exactly the polygons that hold the point and `intersecting`
/// never drops a polygon whose exact box meets the query.
#[allow(clippy::too_many_arguments, reason = "the polygons case's state")]
fn poly_index_checks(
    c: &mut Cursor<'_>,
    v: &Polygons<'_>,
    a: &Arcs<'_>,
    topo: &Topo,
    checked: bool,
    model: bool,
    p: &Pack<'_>,
    probe: impl Fn(&mut Cursor<'_>) -> Point<E7>,
) {
    let Ok(ix) = PolyIndex::open(&section(p, Kind::POLY_INDEX), v) else {
        assert!(!model, "a written POLY_INDEX payload does not open");
        return;
    };
    let n = v.len();
    let random = c.below(n + 2)..c.below(n + 2);
    let ranges = [
        0..n,
        v.group(0).unwrap_or(0..0),
        v.group(1).unwrap_or(0..0),
        1..n.saturating_sub(1),
        random,
    ];
    let query = |c: &mut Cursor<'_>| {
        let (s, t) = (probe(c), probe(c));
        let (s, t) = (
            Rect::around_e7(s.lat(), s.lon()),
            Rect::around_e7(t.lat(), t.lon()),
        );
        let mut r = s;
        r.grow(&t);
        r
    };
    for range in ranges {
        let q = query(c);
        let mut got = Vec::new();
        let _ = ix.intersecting(q, range.clone(), |i| {
            got.push(i);
            ControlFlow::Continue(())
        });
        assert!(
            got.iter().all(|i| range.contains(i)),
            "a polygon outside the range"
        );
        let at = probe(c);
        let mut held = Vec::new();
        let _ = ix.containing(v, a, at, range.clone(), |i| {
            held.push(i);
            ControlFlow::Continue(())
        });
        if !(checked && ix.check(v, a).is_ok()) {
            continue;
        }
        got.sort_unstable();
        let len = got.len();
        got.dedup();
        assert_eq!(got.len(), len, "a polygon reported twice");
        let inside = range.start..range.end.min(n);
        for i in inside.clone() {
            if polygon_box(v, a, i).is_some_and(|b| b.intersects(&q)) {
                assert!(
                    got.binary_search(&i).is_ok(),
                    "polygon {i} dropped from {q:?}"
                );
            }
        }
        held.sort_unstable();
        let want: Vec<usize> = inside.filter(|&i| v.contains(a, i, at)).collect();
        assert_eq!(held, want, "containing {at} in {range:?}");
    }
    if model {
        assert!(checked, "the model polygons fail their check");
        assert_eq!(
            ix.check(v, a),
            Ok(()),
            "a written POLY_INDEX fails its check"
        );
        assert_eq!(ix.len(), topo.polys.len());
    }
}
