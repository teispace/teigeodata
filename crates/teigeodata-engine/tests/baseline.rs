//! The embedded baseline (feature `baseline`): the file `forge embed` builds, compiled in.
//!
//! ```text
//! TEIGEODATA_BASELINE=$PWD/data/build/baseline/baseline-main.tgd \
//!   cargo test --release -p teigeodata-engine --features baseline --test baseline
//! ```

#![allow(
    clippy::unwrap_used,
    reason = "tests: failures panic with their message"
)]

use teigeodata_core::{E7, PlaceKind, Point};
use teigeodata_engine::{BASELINE, Engine, Place, Scratch};

fn name(e: &Engine, p: &Place) -> String {
    e.name(p, &mut [0; 512])
        .unwrap()
        .unwrap_or_default()
        .to_owned()
}

fn at(lat: f64, lon: f64) -> Point<E7> {
    Point::<E7>::from_degrees(lat, lon).unwrap()
}

/// Z1 (D-144): the embedded file fits its 8 MB budget.
#[test]
fn the_baseline_fits_its_budget() {
    assert!(BASELINE.len() <= 8_000_000, "{} B", BASELINE.len());
}

/// It answers the world: a country, its unit and zone at a point, and a city by name, in a
/// country with a pack's slice (Nepal) and one without (France).
#[test]
fn the_baseline_answers_the_world() {
    let e = Engine::with_baseline().unwrap();
    let (mut s, mut units, mut hits) = (Scratch::default(), Vec::new(), Vec::new());
    for (lat, lon, country, zone) in [
        (27.7172, 85.3240, "Nepal", "Asia/Kathmandu"),
        (48.8566, 2.3522, "France", "Europe/Paris"),
    ] {
        e.admin_at(at(lat, lon), &mut s, &mut units).unwrap();
        let found: Vec<_> = units.iter().map(|u| (u.kind, name(&e, u))).collect();
        assert!(
            found.contains(&(Some(PlaceKind::Country), country.to_owned())),
            "{found:?}"
        );
        assert!(
            units.iter().any(|u| u.kind == Some(PlaceKind::Admin)),
            "a unit at {lat}, {lon}: {found:?}"
        );
        let tz = e.tz_at(at(lat, lon)).unwrap().unwrap();
        assert_eq!(name(&e, &tz), zone);
    }
    for city in ["Kathmandu", "Paris", "Pokhara", "Tokyo"] {
        e.search(city, 5, &mut s, &mut hits).unwrap();
        let top = hits.first().map(|h| name(&e, &h.place));
        assert_eq!(top.as_deref(), Some(city), "{city}");
    }
}
