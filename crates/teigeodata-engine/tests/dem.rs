//! Terrain at any point (D-192): `elevation_at` answers from the first layer whose DEM has
//! samples around the point, falls through layers whose DEM does not, reads as the DEM reader
//! does, and refuses a layer whose DEM is not installed. Where no fine DEM answers, the coarse
//! grid does (D-225), and the answer states its spacing.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "tests: failures panic with their message; values are small"
)]

use teigeodata_core::{E7, Point};
use teigeodata_engine::{
    Component, Components, Datum, DemSource, Engine, Error, Layer, LayerOptions, Scratch, Verify,
};
use teigeodata_format::dem::{
    self, DATUM_EGM2008, Dem, DemScratch, Family, Grid, GridInput, THRESHOLDS, TileOptions,
};
use teigeodata_format::{Encoding, Kind, Pack, Writer};

/// Tile row and column of the global 64-sample tile grid near 28° N, 84° E.
const ROW0: u32 = 3_496;
const COL0: u32 = 14_400;

/// A DEM pack at `address`, made from NASADEM when the address names a part (`np-p4`) and
/// from Copernicus GLO-30 otherwise, of one grid of 1 × `cols` tiles at (`ROW0`, `COL0`), tile `c` at height
/// `base + c·100 m` sloping east, `absent` tiles left out.
fn dem_pack(address: &str, cols: u32, base: i32, absent: &[u32]) -> Vec<u8> {
    let family = Family::standard();
    let codes: Vec<Option<Vec<u8>>> = (0..cols)
        .map(|c| {
            (!absent.contains(&c)).then(|| {
                let q: Vec<i32> = (0..4096)
                    .map(|i| base / 2 + 50 * c.cast_signed() + (i % 64) / 8)
                    .collect();
                dem::encode_tile(&q, 6, &family, &THRESHOLDS, &TileOptions::default()).unwrap()
            })
        })
        .collect();
    let tiles: Vec<Option<&[u8]>> = codes.iter().map(Option::as_deref).collect();
    let grid = Grid {
        tile_row0: ROW0,
        tile_col0: COL0,
        tile_rows: 1,
        tile_cols: cols,
        lat_step: 10,
        lon_step: 10,
        step_mm: 2_000,
        datum: DATUM_EGM2008,
        source: if address.contains('-') {
            dem::Source::Nasadem.code()
        } else {
            dem::Source::CopernicusGlo30.code()
        },
        tile_log2: 6,
        first_tile: 0,
    };
    let body = dem::encode(
        &[GridInput {
            grid,
            tiles: &tiles,
        }],
        &family,
        &THRESHOLDS,
    )
    .unwrap();
    Writer::new(address, "places", "2026.11.0")
        .unwrap()
        .pack(
            &[(Kind::DEM, Encoding::TABLE, u64::from(cols), &body)],
            None,
        )
        .unwrap()
}

/// A point inside tile column `c`, at sample (10.5, 20.25) of it.
fn at(c: u32) -> Point<E7> {
    let r = f64::from(ROW0) * 64.0 + 10.5;
    let col = f64::from(COL0 + c) * 64.0 + 20.25;
    let lat = 90.0 - r / 3600.0;
    let lon = -180.0 + col / 3600.0;
    Point::<E7>::from_units((lat * 1e7).round() as i32, (lon * 1e7).round() as i32).unwrap()
}

/// What the DEM reader alone gives at `p`.
fn direct(bytes: &[u8], p: Point<E7>) -> Option<f64> {
    let pack = Pack::open(bytes).unwrap();
    let s = pack.section(Kind::DEM).unwrap();
    Dem::open(&s).unwrap().elevation(p, &mut DemScratch::new())
}

fn layer(bytes: Vec<u8>, installed: Option<Components>) -> Layer {
    let options = LayerOptions {
        installed,
        verify: Verify::Lazy,
        ..LayerOptions::default()
    };
    Layer::from_bytes(bytes, options).unwrap()
}

#[test]
fn heights_come_from_the_first_layer_with_terrain_there() {
    // The upper layer (a province, more specific) has tiles 0 and 2 (1 absent); the lower
    // one (the country) 0–3, 2,000 m higher.
    let upper = dem_pack("np-p4", 3, 500, &[1]);
    let lower = dem_pack("np", 4, 2_500, &[]);
    let mut e = Engine::new();
    e.add(layer(upper.clone(), None));
    e.add(layer(lower.clone(), None));
    let mut s = Scratch::default();
    let ids: Vec<_> = e.layers().map(Layer::id).collect();
    // Each height says where it came from: the answering layer's product, spacing and datum.
    for c in [0, 2] {
        let got = e.elevation_at(at(c), &mut s).unwrap().unwrap();
        assert_eq!(Some(got.meters), direct(&upper, at(c)), "tile {c}");
        assert!(got.meters < 1_000.0);
        assert_eq!(got.source, Some(DemSource::Nasadem));
        assert_eq!(got.layer, ids[0]);
    }
    // Absent above, present below; beyond the upper grid; beyond both.
    for c in [1, 3] {
        let got = e.elevation_at(at(c), &mut s).unwrap().unwrap();
        assert_eq!(Some(got.meters), direct(&lower, at(c)), "tile {c}");
        assert!(got.meters > 2_500.0);
        assert_eq!(got.source, Some(DemSource::CopernicusGlo30));
        assert_eq!((got.datum, got.layer), (Datum::Egm2008, ids[1]));
        // 1″ of latitude.
        assert!(
            (got.resolution_m - 30.92).abs() < 0.01,
            "{}",
            got.resolution_m
        );
    }
    assert_eq!(e.elevation_at(at(9), &mut s).unwrap(), None);
    // The same answers again from the warm scratch, alternating layers.
    for c in [0, 1, 2, 3, 0] {
        let want = if c % 2 == 0 {
            direct(&upper, at(c))
        } else {
            direct(&lower, at(c))
        };
        assert_eq!(
            e.elevation_at(at(c), &mut s).unwrap().map(|h| h.meters),
            want
        );
    }
}

#[test]
fn a_dem_that_is_not_installed_is_named() {
    let bytes = dem_pack("np", 2, 500, &[]);
    let mut e = Engine::new();
    let without = Components::of(&[Component::Point]);
    e.add(layer(bytes, Some(without)));
    match e.elevation_at(at(0), &mut Scratch::default()) {
        Err(Error::MissingComponent { component, .. }) => assert_eq!(component, Component::Dem),
        other => panic!("{other:?}"),
    }
}

/// The DEM section of a pack from [`dem_pack`].
fn dem_body(pack: &[u8]) -> (Vec<u8>, u64) {
    let p = Pack::open(pack).unwrap();
    let s = p.section(Kind::DEM).unwrap();
    (s.bytes().to_vec(), s.item_count())
}

/// A pack with the given DEM and coarse sections (either may be left out).
fn terrain_pack(fine: Option<&(Vec<u8>, u64)>, coarse: Option<&(Vec<u8>, u64)>) -> Vec<u8> {
    let mut sections: Vec<(Kind, Encoding, u64, &[u8])> = Vec::new();
    if let Some((b, n)) = fine {
        sections.push((Kind::DEM, Encoding::TABLE, *n, b));
    }
    if let Some((b, n)) = coarse {
        sections.push((Kind::DEM_COARSE, Encoding::TABLE, *n, b));
    }
    Writer::new("np", "places", "2026.11.0")
        .unwrap()
        .pack(&sections, None)
        .unwrap()
}

/// What the coarse reader alone gives at `p`.
fn coarse_direct(body: &(Vec<u8>, u64), p: Point<E7>) -> Option<f64> {
    let s = teigeodata_format::Section::unpacked(Kind::DEM_COARSE, body.1, &body.0);
    Dem::open(&s).unwrap().elevation(p, &mut DemScratch::new())
}

/// D-225: the fine DEM answers where installed and present; the coarse grid where it is not
/// installed, absent there, or not in the pack; each answer states its spacing.
#[test]
fn the_coarse_grid_answers_where_no_fine_dem_does() {
    let fine = dem_body(&dem_pack("np", 4, 1_500, &[1]));
    // The coarse grid of the whole DEM, 4″ cells.
    let whole = dem_body(&dem_pack("np", 4, 1_500, &[]));
    let c = dem::Coarse {
        factor: 4,
        step_mm: 2_000,
        tile_log2: 4,
    };
    let src = Dem::open(&teigeodata_format::Section::unpacked(
        Kind::DEM,
        whole.1,
        &whole.0,
    ))
    .unwrap();
    let coarse = dem::coarsen(
        &src,
        c,
        &Family::standard(),
        &THRESHOLDS,
        &TileOptions::default(),
    )
    .unwrap()
    .unwrap();
    let both = terrain_pack(Some(&fine), Some(&coarse));
    let fine_m = 30.92;
    let coarse_m = 4.0 * 30.92;
    let near = |r: f64, want: f64| (r - want).abs() < 0.05;
    let mut s = Scratch::default();
    let ask = |installed: Option<Components>, bytes: &[u8], c: u32, s: &mut Scratch| {
        let mut e = Engine::new();
        e.add(layer(bytes.to_vec(), installed));
        e.elevation_at(at(c), s)
    };
    // Both installed: the fine DEM where it has samples, the coarse grid in its absent tile.
    let h = ask(None, &both, 0, &mut s).unwrap().unwrap();
    assert!(near(h.resolution_m, fine_m), "{}", h.resolution_m);
    assert_eq!(Some(h.meters), direct(&both, at(0)));
    let h = ask(None, &both, 1, &mut s).unwrap().unwrap();
    assert!(near(h.resolution_m, coarse_m), "{}", h.resolution_m);
    assert_eq!(Some(h.meters), coarse_direct(&coarse, at(1)));
    assert_eq!(h.source, Some(DemSource::CopernicusGlo30));
    // The coarse sample is its cell's mean: near the fine height, not equal to it.
    let fine_there = direct(&terrain_pack(Some(&whole), None), at(1)).unwrap();
    assert!(
        (h.meters - fine_there).abs() < 10.0,
        "{} vs {fine_there}",
        h.meters
    );
    // Only the coarse grid installed: it answers everywhere it has samples.
    let only = Some(Components::of(&[Component::DemCoarse]));
    for c in 0..4 {
        let h = ask(only, &both, c, &mut s).unwrap().unwrap();
        assert!(near(h.resolution_m, coarse_m));
        assert_eq!(Some(h.meters), coarse_direct(&coarse, at(c)));
    }
    // Only the fine DEM installed: its absent tile has no terrain.
    let fine_only = Some(Components::of(&[Component::Dem]));
    let h = ask(fine_only, &both, 0, &mut s).unwrap().unwrap();
    assert!(near(h.resolution_m, fine_m));
    assert_eq!(ask(fine_only, &both, 1, &mut s).unwrap(), None);
    // A pack with only the coarse grid.
    let coarse_pack = terrain_pack(None, Some(&coarse));
    let h = ask(None, &coarse_pack, 2, &mut s).unwrap().unwrap();
    assert!(near(h.resolution_m, coarse_m));
    // Neither installed: the fine DEM is named.
    let none = Some(Components::of(&[Component::Point]));
    match ask(none, &both, 0, &mut s) {
        Err(Error::MissingComponent { component, .. }) => assert_eq!(component, Component::Dem),
        other => panic!("{other:?}"),
    }
    // Outside both grids: nothing, and no error.
    assert_eq!(ask(only, &both, 9, &mut s).unwrap(), None);
}
