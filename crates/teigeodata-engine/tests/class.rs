//! A place's class (D-381, TG-32): `Engine::class` reads the PLACES class column by the core
//! list, a code the list does not have reads as none, and a pack without the column has none.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message"
)]

mod common;

use common::synthetic;
use teigeodata_core::class::class_code;
use teigeodata_engine::{Engine, Layer, LayerOptions};

fn engine(bytes: Vec<u8>) -> Engine {
    let mut e = Engine::new();
    e.add(Layer::from_bytes(bytes, LayerOptions::default()).unwrap());
    e
}

#[test]
fn each_place_reads_its_class() {
    let mut model = synthetic(200, 11);
    let n = model.ids.len();
    let temple_code = class_code("temple").unwrap();
    let palace_code = class_code("palace").unwrap();
    model.classes = (0..n)
        .map(|r| match r % 5 {
            0 => temple_code,
            1 => palace_code,
            // A code a newer list may have: read as none, not refused.
            2 => 9_999,
            _ => 0,
        })
        .collect();
    let e = engine(model.encode());
    for r in 0..n {
        let place = e.get(model.ids[r]).unwrap().unwrap();
        let want = match r % 5 {
            0 => Some("temple"),
            1 => Some("palace"),
            _ => None,
        };
        assert_eq!(e.class(&place).unwrap(), want, "row {r}");
    }
    // Without the column, no place has a class.
    let plain = synthetic(200, 11);
    let e = engine(plain.encode());
    let place = e.get(plain.ids[0]).unwrap().unwrap();
    assert_eq!(e.class(&place).unwrap(), None);
}
