//! Terrain at any point (D-192): `elevation_at` answers from the first layer whose DEM has
//! samples around the point, falls through layers whose DEM does not, reads as the DEM reader
//! does, and refuses a layer whose DEM is not installed.

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
