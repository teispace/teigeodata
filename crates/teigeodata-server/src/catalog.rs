//! The store's catalog as the servers and `tgd` answer it (feature `store`, D-318): a catalog
//! entry as a row, and `coverage`, the countries at or near a point with the packs that could
//! hold it. Read from the stored, verified catalog without opening the store
//! ([`teigeodata_sync::client::catalog_at`]), so it runs offline beside an install.

use serde_json::{Value, json};
use teigeodata_core::{E7, Point};
use teigeodata_engine::{Components, Engine, Error, Place};
use teigeodata_sync::catalog::{Catalog, Entry, PartSize};
use teigeodata_sync::client::{PackState, language};
use teigeodata_sync::store::Record;

/// A catalog entry as a row: path, release, download and disk bytes of the whole pack, its
/// components and parts, licence and `state` against the store.
#[must_use]
pub fn entry(e: &Entry, state: PackState) -> Value {
    let all = e.size_of(Components::all());
    json!({
        "path": e.path(),
        "release": e.release,
        "download": all.wire,
        "disk": all.disk,
        "components": e.sizes.keys().map(|c| c.name()).collect::<Vec<_>>(),
        "parts": e.parts.iter().map(part).collect::<Vec<_>>(),
        "licence": e.licence,
        "state": state.name(),
    })
}

/// A catalog part: its component, bytes, and its language or its outline's level, level of
/// detail and tolerance.
fn part(p: &PartSize) -> Value {
    let (component, download, disk) = (p.component.name(), p.size.wire, p.size.disk);
    p.part.outline_levels().map_or_else(
        || json!({"component": component, "language": language(p.part), "download": download, "disk": disk}),
        |(level, lod)| json!({"component": component, "level": level, "lod": lod, "tolerance_m": p.tolerance_m, "download": download, "disk": disk}),
    )
}

/// The countries at `at` or within `radius_m` metres of it (the engine's locator), each with
/// the packs of `catalog` that could hold the point and their state against `installed`, and
/// the countries' places (for their attribution). A country with no code is left out.
///
/// # Errors
/// The engine's (a corrupt section).
pub fn coverage(
    engine: &Engine,
    catalog: &Catalog,
    installed: &[Record],
    at: Point<E7>,
    radius_m: u32,
) -> Result<(Vec<Value>, Vec<Place>), Error> {
    let mut countries: Vec<Place> = Vec::new();
    engine.countries_near(at, radius_m, &mut countries)?;
    let mut buf = [0_u8; crate::output::NAME];
    let mut rows = Vec::with_capacity(countries.len());
    countries.retain(|c| c.id.namespace().country().is_some());
    for country in &countries {
        let Some(cc) = country.id.namespace().country() else {
            continue;
        };
        rows.push(json!({
            "id": country.id.to_string(),
            "name": crate::output::installed(engine.name(country, &mut buf))?,
            "lat": country.point.lat_degrees(),
            "lon": country.point.lon_degrees(),
            "countries": [String::from_utf8_lossy(&cc)],
            "packs": catalog
                .packs_at(cc, at)
                .map(|e| entry(e, PackState::of(e, installed)))
                .collect::<Vec<_>>(),
        }));
    }
    Ok((rows, countries))
}
