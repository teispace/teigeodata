//! Cargo features per component (D-137): under any set of the engine's features, a component
//! whose feature is off counts as not installed. A whole pack opens, its sections of that
//! component are never read, and the components it reports are the built ones it has. Run
//! with the default features and with `--no-default-features` (CI runs both).

#![allow(
    clippy::unwrap_used,
    reason = "tests: failures panic with their message"
)]

mod common;

use common::synthetic;
use teigeodata_engine::{Component, Components, Engine, Layer, LayerOptions, Scratch, Verify};

/// The components whose feature this test binary was built with (`point` always).
fn built() -> Components {
    let on = [
        (Component::Elevation, cfg!(feature = "elevation")),
        (Component::Admin, cfg!(feature = "admin")),
        (Component::Names, cfg!(feature = "names")),
        (Component::Search, cfg!(feature = "search")),
        (Component::Timezone, cfg!(feature = "timezone")),
        (Component::TimezoneIndex, cfg!(feature = "timezone-index")),
        (Component::Boundaries, cfg!(feature = "boundaries")),
        (Component::AdminIndex, cfg!(feature = "admin-index")),
    ];
    let off: Vec<Component> = on.iter().filter(|c| !c.1).map(|c| c.0).collect();
    Components::all().without(Components::of(&off))
}

#[test]
fn a_component_without_its_feature_is_not_installed() {
    let model = synthetic(400, 5);
    let bytes = model.encode();
    for verify in [Verify::Eager, Verify::Lazy] {
        let options = LayerOptions {
            verify,
            ..LayerOptions::default()
        };
        let layer = Layer::from_bytes(bytes.clone(), options).unwrap();
        let has = layer.components();
        // The synthetic pack has every component the engine reads but `admin` and outlines.
        let pack = Components::of(&[
            Component::Point,
            Component::Elevation,
            Component::Names,
            Component::Search,
            Component::Timezone,
            Component::TimezoneIndex,
            Component::AdminIndex,
        ]);
        assert_eq!(has, pack.without(Components::all().without(built())));
        // `point` answers under every feature set.
        let mut e = Engine::new();
        e.add(layer);
        let mut s = Scratch::default();
        let mut near = Vec::new();
        for (&id, p) in model.ids.iter().zip(&model.points).take(50) {
            assert_eq!(e.get(id).unwrap().map(|p| p.id), Some(id));
            e.reverse(*p, 1, &mut s, &mut near).unwrap();
            assert_eq!(near.first().map(|n| n.meters < 1.0), Some(true));
        }
    }
}
