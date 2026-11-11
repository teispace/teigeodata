//! The queries: `get`, `reverse`, `near`, `search`, `admin` and `tz`, over the packs given with
//! `--pack` as one dataset (the engine's layers, D-113), printed by [`crate::output`].
//!
//! The files are read into memory: the CLI does not own them (D-113). A query with no result
//! prints its empty output and exits 1.

use std::io::Write;
use std::path::PathBuf;

use teigeodata_core::{E6, E7, PlaceId, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Load, Scratch};

use crate::Fail;
use crate::output::{self, Item, Mode};

/// The engine over `packs`, in the order given (equal priority: later wins ties by the
/// engine's rules).
pub(crate) fn engine(packs: &[PathBuf]) -> Result<Engine, Fail> {
    let mut e = Engine::new();
    for p in packs {
        let options = LayerOptions {
            load: Load::Read,
            ..LayerOptions::default()
        };
        e.add(Layer::open(p, options).map_err(|x| Fail::of(p.display(), x))?);
    }
    Ok(e)
}

/// A point from degrees, on grid `G`.
pub(crate) fn point<G>(lat: f64, lon: f64) -> Result<Point<G>, Fail>
where
    G: teigeodata_core::Grid,
{
    Point::from_degrees(lat, lon).map_err(|e| Fail::Refused(format!("{lat},{lon}: {e}")))
}

fn id(text: &str) -> Result<PlaceId, Fail> {
    text.parse()
        .map_err(|e| Fail::Refused(format!("{text}: {e}")))
}

/// What a query asks.
#[derive(Debug)]
pub(crate) enum Ask {
    Get {
        id: String,
    },
    Reverse {
        lat: f64,
        lon: f64,
        k: usize,
    },
    Near {
        id: String,
        k: usize,
        radius: Option<f64>,
    },
    Search {
        query: String,
        k: usize,
    },
    Admin {
        lat: f64,
        lon: f64,
    },
    TzAt {
        lat: f64,
        lon: f64,
    },
    TzOf {
        id: String,
    },
}

pub(crate) fn run(
    store: &std::path::Path,
    packs: &[PathBuf],
    ask: Ask,
    mode: Mode,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let e = if packs.is_empty() {
        crate::sync::Opened::open(store)?.engine(&[])?
    } else {
        engine(packs)?
    };
    let mut s = Scratch::default();
    let items: Vec<Item> = match ask {
        Ask::Get { id: text } => {
            let place = e.get(id(&text)?)?;
            vec![
                place
                    .ok_or_else(|| Fail::Refused(format!("{text}: not found")))?
                    .into(),
            ]
        }
        Ask::Reverse { lat, lon, k } => {
            let mut near = Vec::new();
            e.reverse(point::<E6>(lat, lon)?, k, &mut s, &mut near)?;
            near.iter()
                .map(|n| Item {
                    place: n.place,
                    meters: Some(n.meters),
                    score: None,
                })
                .collect()
        }
        Ask::Near {
            id: text,
            k,
            radius,
        } => {
            let place = e
                .get(id(&text)?)?
                .ok_or_else(|| Fail::Refused(format!("{text}: not found")))?;
            let mut near = Vec::new();
            match radius {
                Some(m) => {
                    // The place itself is at 0 m: ask for one more and drop it.
                    e.within_radius(place.point, m, k.saturating_add(1), &mut s, &mut near)?;
                    near.retain(|n| n.place.id != place.id);
                    near.truncate(k);
                }
                None => e.nearby(&place, k, &mut s, &mut near)?,
            }
            near.iter()
                .map(|n| Item {
                    place: n.place,
                    meters: Some(n.meters),
                    score: None,
                })
                .collect()
        }
        Ask::Search { query, k } => {
            let mut hits = Vec::new();
            e.search(&query, k, &mut s, &mut hits)?;
            hits.iter()
                .map(|h| Item {
                    place: h.place,
                    meters: None,
                    score: Some(h.score),
                })
                .collect()
        }
        Ask::Admin { lat, lon } => {
            let mut units = Vec::new();
            e.admin_at(point::<E7>(lat, lon)?, &mut s, &mut units)?;
            units.into_iter().map(Item::from).collect()
        }
        Ask::TzAt { lat, lon } => e
            .tz_at(point::<E7>(lat, lon)?)?
            .into_iter()
            .map(Item::from)
            .collect(),
        Ask::TzOf { id: text } => {
            let place = e
                .get(id(&text)?)?
                .ok_or_else(|| Fail::Refused(format!("{text}: not found")))?;
            e.tz_of(&place)?.into_iter().map(Item::from).collect()
        }
    };
    output::write(&e, &items, mode, out)?;
    // As grep: no result is exit 1, after the (empty) output a script may still parse.
    if items.is_empty() {
        return Err(Fail::Refused("no result".into()));
    }
    Ok(())
}

/// `tgd elevation`: the terrain height at a point from the packs' DEM (D-192), in metres
/// above EGM2008 to a tenth; no terrain there is exit 1.
pub(crate) fn elevation(
    store: &std::path::Path,
    packs: &[PathBuf],
    (lat, lon): (f64, f64),
    json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let e = if packs.is_empty() {
        crate::sync::Opened::open(store)?.engine(&[])?
    } else {
        engine(packs)?
    };
    let h = e.elevation_at(point::<E7>(lat, lon)?, &mut Scratch::default())?;
    match (h, json) {
        (Some(m), false) => writeln!(out, "{m:.1} m above EGM2008")?,
        (Some(m), true) => writeln!(
            out,
            "{{\"lat\":{lat},\"lon\":{lon},\"elevation_m\":{m:.1},\"datum\":\"EGM2008\"}}"
        )?,
        (None, true) => writeln!(
            out,
            "{{\"lat\":{lat},\"lon\":{lon},\"elevation_m\":null,\"datum\":\"EGM2008\"}}"
        )?,
        (None, false) => {}
    }
    match h {
        Some(_) => Ok(()),
        None => Err(Fail::Refused("no terrain there".into())),
    }
}
