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

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::blobs::{self, Blobs};
use crate::hierarchy::{self, Hierarchy, MAX_DEPTH};
use crate::idmap::{self, IdMap};
use crate::place_index::{self, MAX_NODE, MIN_NODE, Neighbour, PlaceIndex, Rect};
use crate::places::{self, PlaceColumns, Places};
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
    match mode % 8 {
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
/// or a vertex already drawn (shared borders and junctions).
fn region_vertex(c: &mut Cursor<'_>, seen: &[[i32; 2]]) -> [i32; 2] {
    match c.u8() & 7 {
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
    let mut v = region_vertex(c, seen);
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
    let regions: Vec<Vec<Vec<[i32; 2]>>> = (0..c.below(7))
        .map(|_| {
            (0..=c.below(2))
                .map(|_| {
                    (0..3 + c.below(limit(10, 4)))
                        .map(|_| {
                            let v = region_vertex(c, &seen);
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
                core::ops::ControlFlow::Continue(())
            });
        }
        let mut best = Vec::new();
        let floor = c.u8();
        let read = ix.best_in(
            range.clone(),
            || i32::from(floor),
            |o| {
                best.push(o);
                core::ops::ControlFlow::Continue(())
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
                    core::ops::ControlFlow::Continue(())
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
                    core::ops::ControlFlow::Continue(())
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
            core::ops::ControlFlow::Continue(())
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

/// A model's rows, in Hilbert order: point, kind, admin level, class, flags.
type PlaceRow = (Point<E6>, PlaceKind, u8, u32, u32);

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
                (p, kind, level, u32::from(c.u8() % 5), u32::from(c.u8() & 3))
            })
            .collect();
        rows.sort_by_key(|r| hilbert(r.0));
        let points: Vec<_> = rows.iter().map(|r| r.0).collect();
        let kinds: Vec<_> = rows.iter().map(|r| r.1).collect();
        let on = |bit: u8| optional & bit != 0;
        let levels: Vec<u8> = rows.iter().filter(|_| on(1)).map(|r| r.2).collect();
        let classes: Vec<u32> = rows.iter().filter(|_| on(2)).map(|r| r.3).collect();
        let flags: Vec<u32> = rows.iter().filter(|_| on(4)).map(|r| r.4).collect();
        let cols = PlaceColumns {
            points: &points,
            kinds: &kinds,
            admin_levels: &levels,
            classes: &classes,
            flags: &flags,
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
        )
    };
    for r in rows(v.len()) {
        let _ = read(r);
    }
    let checked = v.check().is_ok();
    if let Some(m) = &model {
        assert!(checked, "a written PLACES payload fails its check");
        assert_eq!(v.len(), m.len());
        for (r, &(pt, kind, level, class, flags)) in m.iter().enumerate() {
            let want = (Some(pt), Some(kind), Some(level), Some(class), Some(flags));
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
        assert_eq!(read(v.len()), (None, None, None, None, None));
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
