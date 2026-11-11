//! DEM (D-089): tiles round-trip exactly on synthetic terrain (slopes, ridges, cliffs that
//! escape, flat lakes, noise), point queries interpolate the decoded samples, absent tiles and
//! the coverage edge read as described, and every edit of a checked value is refused.

#![cfg(feature = "write")]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    reason = "test code: model values are small, indices come from the same vectors"
)]

use teigeodata_core::table::Table;
use teigeodata_core::{E7, Point};
use teigeodata_format::dem::{
    self, DATUM_EGM2008, Dem, DemScratch, Family, Grid, GridInput, OFFSETS, PAYLOAD, THRESHOLDS,
    TileOptions,
};
use teigeodata_format::{Kind, Section};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> i64 {
        (self.next() % n) as i64
    }
}

/// Terrain at global sample (r, c), in 2 m steps: a tilted plane, ridges, a cliff band, a flat
/// lake and noise.
fn terrain(r: i64, c: i64, rng: &mut Rng) -> i32 {
    let (x, y) = (c as f64, r as f64);
    let lake = (x - 300.0).powi(2) + (y - 120.0).powi(2) < 900.0;
    if lake {
        return 700;
    }
    let mut h = 400.0 + 0.8 * x - 0.3 * y + 60.0 * (x / 17.0).sin() * (y / 23.0).cos();
    if (c / 40) % 5 == 3 {
        h += 3_000.0; // cliffs: residuals far beyond 62 steps
    }
    h as i32 + rng.below(3) as i32 - 1
}

const LOG2: u32 = 6;
const SIDE: i64 = 1 << LOG2;

/// A grid, its tiles' codes and their samples.
type Built = (Grid, Vec<Option<Vec<u8>>>, Vec<Option<Vec<i32>>>);

/// A grid of `tile_rows × tile_cols` tiles at tile (row0, col0), `absent` tiles left out.
fn grid_codes(
    row0: u32,
    col0: u32,
    tile_rows: u32,
    tile_cols: u32,
    absent: &[usize],
    rng: &mut Rng,
) -> Built {
    let family = Family::standard();
    let mut codes = Vec::new();
    let mut truth = Vec::new();
    for t in 0..(tile_rows * tile_cols) as usize {
        let (tr, tc) = (
            t as i64 / i64::from(tile_cols),
            t as i64 % i64::from(tile_cols),
        );
        if absent.contains(&t) {
            codes.push(None);
            truth.push(None);
            continue;
        }
        let q: Vec<i32> = (0..SIDE * SIDE)
            .map(|i| {
                let r = (i64::from(row0) + tr) * SIDE + i / SIDE;
                let c = (i64::from(col0) + tc) * SIDE + i % SIDE;
                terrain(r - i64::from(row0) * SIDE, c - i64::from(col0) * SIDE, rng)
            })
            .collect();
        codes.push(Some(
            dem::encode_tile(&q, LOG2, &family, &THRESHOLDS, &TileOptions::default()).unwrap(),
        ));
        truth.push(Some(q));
    }
    let grid = Grid {
        tile_row0: row0,
        tile_col0: col0,
        tile_rows,
        tile_cols,
        lat_step: 10,
        lon_step: 10,
        step_mm: 2_000,
        datum: DATUM_EGM2008,
        source: 0,
        tile_log2: LOG2,
        first_tile: 0,
    };
    (grid, codes, truth)
}

fn section(grid: Grid, codes: &[Option<Vec<u8>>]) -> Vec<u8> {
    let tiles: Vec<Option<&[u8]>> = codes.iter().map(Option::as_deref).collect();
    dem::encode(
        &[GridInput {
            grid,
            tiles: &tiles,
        }],
        &Family::standard(),
        &THRESHOLDS,
    )
    .unwrap()
}

fn open(body: &[u8], tiles: usize) -> Dem<'_> {
    Dem::open(&Section::unpacked(Kind::DEM, tiles as u64, body)).unwrap()
}

/// Nepal-like placement: tile row 974 from 90° N is 28.0° − …, column 4,078 east of 180° W.
const ROW0: u32 = 974;
const COL0: u32 = 4_078;

#[test]
fn tiles_round_trip_exactly() {
    let mut rng = Rng(42);
    let (grid, codes, truth) = grid_codes(ROW0, COL0, 3, 8, &[5, 17], &mut rng);
    let body = section(grid, &codes);
    let d = open(&body, 24);
    d.check().unwrap();
    assert_eq!(d.grid(0).unwrap(), grid);
    let mut s = DemScratch::new();
    for (t, want) in truth.iter().enumerate() {
        let got = d.decode_tile(t, &mut s).map(<[i32]>::to_vec);
        assert_eq!(got.as_ref(), want.as_ref(), "tile {t}");
    }
    // Deterministic: the same input gives the same bytes.
    let (grid2, codes2, _) = grid_codes(ROW0, COL0, 3, 8, &[5, 17], &mut Rng(42));
    assert_eq!(section(grid2, &codes2), body);
}

#[test]
fn a_flat_tile_is_its_header() {
    let family = Family::standard();
    let q = vec![1234; 4096];
    let code = dem::encode_tile(&q, LOG2, &family, &THRESHOLDS, &TileOptions::default()).unwrap();
    // Base (2 bytes), coefficients 5, models 6, state 4.
    assert_eq!(code.len(), 17, "{code:?}");
}

/// The height at a global sample position (r, c) plus fractions, against the samples.
fn point(r: f64, c: f64) -> Point<E7> {
    let lat = 90.0 - r / 3600.0;
    let lon = -180.0 + c / 3600.0;
    Point::<E7>::from_units((lat * 1e7).round() as i32, (lon * 1e7).round() as i32).unwrap()
}

#[test]
fn queries_interpolate_the_samples() {
    let mut rng = Rng(7);
    let (grid, codes, truth) = grid_codes(ROW0, COL0, 2, 3, &[4], &mut rng);
    let body = section(grid, &codes);
    let d = open(&body, 6);
    let at = |tr: i64, tc: i64, i: i64| -> Option<f64> {
        truth[(tr * 3 + tc) as usize]
            .as_ref()
            .map(|q| f64::from(q[i as usize]) * 2.0)
    };
    let mut s = DemScratch::new();
    let mut checked = 0;
    for _ in 0..4_000 {
        // A point strictly inside a cell of samples, away from the absent tile.
        let tr = rng.below(2);
        let tc = rng.below(3);
        if tr * 3 + tc == 4 {
            continue;
        }
        let (y, x) = (rng.below(SIDE as u64 - 1), rng.below(SIDE as u64 - 1));
        let (fy, fx) = (
            rng.below(1000) as f64 / 1000.0,
            rng.below(1000) as f64 / 1000.0,
        );
        let r = (i64::from(ROW0) + tr) * SIDE + y;
        let c = (i64::from(COL0) + tc) * SIDE + x;
        let p = point(r as f64 + fy, c as f64 + fx);
        let got = d.elevation(p, &mut s).unwrap();
        // The point snapped to 1e-7°: recompute its fractions as the reader does.
        let num_r = (900_000_000 - i64::from(p.lat())) * 36;
        let num_c = (1_800_000_000 + i64::from(p.lon())) * 36;
        let (rr, fr) = (num_r / 100_000, (num_r % 100_000) as f64 / 1e5);
        let (cc, fc) = (num_c / 100_000, (num_c % 100_000) as f64 / 1e5);
        let local = |rr: i64, cc: i64| {
            let (tr, tc) = (rr / SIDE - i64::from(ROW0), cc / SIDE - i64::from(COL0));
            at(tr, tc, (rr % SIDE) * SIDE + cc % SIDE).unwrap()
        };
        let want = local(rr, cc) * (1.0 - fr) * (1.0 - fc)
            + local(rr, cc + 1) * (1.0 - fr) * fc
            + local(rr + 1, cc) * fr * (1.0 - fc)
            + local(rr + 1, cc + 1) * fr * fc;
        assert!((got - want).abs() < 1e-6, "{p}: {got} against {want}");
        checked += 1;
    }
    assert!(checked > 3_000);
    // Outside the grid: nothing.
    let outside = point(f64::from(ROW0 - 1) * 64.0, f64::from(COL0) * 64.0 + 5.0);
    assert_eq!(d.elevation(outside, &mut s), None);
    // Inside the absent tile: nothing; at its edge, the present neighbours alone.
    let absent = point(
        f64::from(ROW0 + 1) * 64.0 + 10.5,
        f64::from(COL0 + 1) * 64.0 + 10.5,
    );
    assert_eq!(d.elevation(absent, &mut s), None);
    let edge_r = f64::from(ROW0 + 1) * 64.0 + 10.0;
    let edge_c = f64::from(COL0 + 1) * 64.0 - 1.0 + 0.5;
    let e = d.elevation(point(edge_r, edge_c), &mut s).unwrap();
    // The point snaps to 1e-7°, so row 11 gets a weight of about 1e-4.
    let (r10, r11) = (
        at(1, 0, 10 * SIDE + 63).unwrap(),
        at(1, 0, 11 * SIDE + 63).unwrap(),
    );
    assert!(
        (e - r10).abs() <= (r11 - r10).abs() * 1e-3 + 1e-9,
        "{e} against {r10}, {r11}"
    );
}

fn raw_columns(body: &[u8]) -> Vec<(u16, teigeodata_core::table::Codec, Vec<u8>)> {
    let t = Table::parse(body).unwrap();
    t.ids()
        .map(|id| {
            let (codec, bytes) = t.column_raw(id).unwrap();
            (id, codec, bytes.to_vec())
        })
        .collect()
}

#[test]
fn edits_are_refused() {
    let mut rng = Rng(3);
    let (grid, codes, _) = grid_codes(ROW0, COL0, 2, 2, &[], &mut rng);
    let body = section(grid, &codes);
    open(&body, 4).check().unwrap();
    let cols = raw_columns(&body);
    let rebuild = |cols: &[(u16, teigeodata_core::table::Codec, Vec<u8>)]| {
        let refs: Vec<_> = cols
            .iter()
            .map(|(i, c, b)| (*i, *c, b.as_slice()))
            .collect();
        teigeodata_core::table::encode_with(&refs).unwrap()
    };
    let refused = |b: &[u8]| {
        Dem::open(&Section::unpacked(Kind::DEM, 4, b))
            .and_then(|d| d.check())
            .is_err()
    };
    // Every payload byte: a flipped bit changes the decode or its end state.
    let p = cols.iter().position(|c| c.0 == PAYLOAD).unwrap();
    let mut caught = 0;
    let total = cols[p].2.len();
    for j in 0..total {
        let mut bad = cols.clone();
        bad[p].2[j] ^= 0x10;
        if refused(&rebuild(&bad)) {
            caught += 1;
        }
    }
    // Padding bytes and a few model-index bits that select an equivalent model may pass.
    assert!(
        caught * 100 >= total * 97,
        "{caught} of {total} payload edits refused"
    );
    // A wrong tile count, and a model family that does not sum to 4096.
    assert!(!refused(&rebuild(&cols)));
    assert!(Dem::open(&Section::unpacked(Kind::DEM, 5, &body)).is_err());
    let m = cols.iter().position(|c| c.0 == dem::MODELS).unwrap();
    let mut bad = cols.clone();
    bad[m].2[200] ^= 1;
    assert!(refused(&rebuild(&bad)));
    // Offsets that do not add up.
    let o = cols.iter().position(|c| c.0 == OFFSETS).unwrap();
    let offs = teigeodata_core::bitpack::encode_auto(&[0, 10, 5, 20, 30]);
    let mut bad = cols.clone();
    bad[o].2 = offs;
    assert!(refused(&rebuild(&bad)));
}

#[test]
fn bad_tiles_are_refused_by_the_writer() {
    let family = Family::standard();
    assert!(dem::encode_tile(&[0; 100], 6, &family, &THRESHOLDS, &TileOptions::default()).is_err());
    assert!(
        dem::encode_tile(
            &vec![0; 1 << 14],
            7,
            &family,
            &THRESHOLDS,
            &TileOptions::default()
        )
        .is_err()
    );
}

/// Decoding resumes where the last query stopped, anywhere in a row (inside the rolling run or
/// at its edges): every answer equals the one from a fresh scratch, which decodes from the start.
#[test]
fn resumed_queries_equal_fresh_ones() {
    let mut rng = Rng(99);
    let (grid, codes, _) = grid_codes(ROW0, COL0, 2, 2, &[], &mut rng);
    let body = section(grid, &codes);
    let d = open(&body, 4);
    let mut s = DemScratch::new();
    let mut asked = 0;
    for _ in 0..20 {
        let (tr, tc) = (rng.below(2), rng.below(2));
        let mut i = rng.below(8);
        while i < SIDE * SIDE - SIDE {
            let (y, x) = (i / SIDE, i % SIDE);
            let r = (i64::from(ROW0) + tr) * SIDE + y;
            let c = (i64::from(COL0) + tc) * SIDE + x;
            let p = point(r as f64 + 0.25, c as f64 + 0.25);
            let resumed = d.elevation(p, &mut s);
            let fresh = d.elevation(p, &mut DemScratch::new());
            assert_eq!(resumed, fresh, "tile ({tr}, {tc}), sample {i}");
            assert!(resumed.is_some());
            asked += 1;
            i += 1 + rng.below(90);
        }
    }
    assert!(asked > 500);
}

#[test]
fn a_mask_keeps_its_tiles_exactly_and_shrinks_the_box() {
    let mut rng = Rng(7);
    // 4 × 6 tiles, tile 9 absent.
    let (grid, codes, truth) = grid_codes(ROW0, COL0, 4, 6, &[9], &mut rng);
    let body = section(grid, &codes);
    let whole = open(&body, 24);
    // Keep tile rows 1–2 and columns 2–4 (global), but not (2, 4).
    let keep = |_: &Grid, r: u32, c: u32| {
        let (r, c) = (r - ROW0, c - COL0);
        (1..=2).contains(&r) && (2..=4).contains(&c) && (r, c) != (2, 4)
    };
    let (masked, count) = dem::mask(&whole, keep).unwrap().unwrap();
    assert_eq!(count, 6, "2 rows × 3 columns");
    let d = open(&masked, 6);
    d.check().unwrap();
    let g = d.grid(0).unwrap();
    assert_eq!(
        (g.tile_row0, g.tile_col0, g.tile_rows, g.tile_cols),
        (ROW0 + 1, COL0 + 2, 2, 3)
    );
    let mut s = DemScratch::new();
    for r in 0..2_usize {
        for c in 0..3_usize {
            let got = d.decode_tile(r * 3 + c, &mut s).map(<[i32]>::to_vec);
            let want = if (r, c) == (1, 2) {
                None
            } else {
                truth[(r + 1) * 6 + c + 2].clone()
            };
            assert_eq!(got, want, "tile ({r}, {c})");
        }
    }
    // A point in a kept tile reads as in the whole section; one in a dropped tile reads none.
    let (mut a, mut b) = (DemScratch::new(), DemScratch::new());
    let inside = point(
        f64::from(ROW0 + 1) * 64.0 + 10.5,
        f64::from(COL0 + 2) * 64.0 + 20.25,
    );
    assert_eq!(d.elevation(inside, &mut a), whole.elevation(inside, &mut b));
    assert!(d.elevation(inside, &mut a).is_some());
    let dropped = point(f64::from(ROW0) * 64.0 + 10.5, f64::from(COL0) * 64.0 + 10.5);
    assert!(whole.elevation(dropped, &mut b).is_some());
    assert_eq!(d.elevation(dropped, &mut a), None);
    // Nothing kept: no section.
    assert!(dem::mask(&whole, |_, _, _| false).unwrap().is_none());
    // The same mask gives the same bytes.
    assert_eq!(dem::mask(&whole, keep).unwrap().unwrap().0, masked);
}

#[test]
fn one_scratch_never_mixes_two_sections_tiles() {
    // Two sections over the same tiles with different terrain.
    let (g1, c1, _) = grid_codes(ROW0, COL0, 1, 1, &[], &mut Rng(1));
    let (g2, mut c2, _) = grid_codes(ROW0, COL0, 1, 1, &[], &mut Rng(99));
    // A flat tile far from the other's heights.
    c2[0] = Some(
        dem::encode_tile(
            &[5_000; 4096],
            LOG2,
            &Family::standard(),
            &THRESHOLDS,
            &TileOptions::default(),
        )
        .unwrap(),
    );
    let (b1, b2) = (section(g1, &c1), section(g2, &c2));
    let (d1, d2) = (open(&b1, 1), open(&b2, 1));
    let at = point(f64::from(ROW0) * 64.0 + 30.5, f64::from(COL0) * 64.0 + 30.5);
    let fresh = |d: &Dem<'_>| d.elevation(at, &mut DemScratch::new());
    let mut shared = DemScratch::new();
    for _ in 0..3 {
        assert_eq!(d1.elevation(at, &mut shared), fresh(&d1));
        assert_eq!(d2.elevation(at, &mut shared), fresh(&d2));
    }
    assert_eq!(fresh(&d2), Some(10_000.0));
    assert_ne!(fresh(&d1), fresh(&d2));
}

/// D-195: a grid states its source product; the code reads back, survives a mask, and a code
/// no reader knows is refused. A section without sources writes no column and reads 0.
#[test]
fn a_grid_states_its_source() {
    let mut rng = Rng(11);
    let (grid, codes, _) = grid_codes(ROW0, COL0, 1, 2, &[], &mut rng);
    let plain = section(grid, &codes);
    assert_eq!(open(&plain, 2).grid(0).unwrap().source, 0);
    let stated = section(
        Grid {
            source: dem::Source::Nasadem.code(),
            ..grid
        },
        &codes,
    );
    assert!(
        stated.len() > plain.len(),
        "the column is written only when stated"
    );
    let d = open(&stated, 2);
    d.check().unwrap();
    let g = d.grid(0).unwrap();
    assert_eq!(dem::Source::from_code(g.source), Some(dem::Source::Nasadem));
    let inside = point(f64::from(ROW0) * 64.0 + 3.5, f64::from(COL0) * 64.0 + 3.5);
    let (h, hg) = d.height(inside, &mut DemScratch::new()).unwrap();
    assert_eq!(Some(h), d.elevation(inside, &mut DemScratch::new()));
    assert_eq!(hg.source, g.source);
    let (masked, _) = dem::mask(&d, |_, _, c| c == COL0).unwrap().unwrap();
    assert_eq!(open(&masked, 1).grid(0).unwrap().source, g.source);
    for s in dem::Source::ALL {
        assert_eq!(dem::Source::from_key(s.key()), Some(s));
        assert_eq!(dem::Source::from_code(s.code()), Some(s));
    }
    let unknown = section(Grid { source: 99, ..grid }, &codes);
    assert!(open(&unknown, 2).check().is_err());
}
