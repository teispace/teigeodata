//! The queries: `get`, `reverse`, `near`, `search`, `admin`, `tz`, `elevation` and `postal`, over
//! the packs given with `--pack` as one dataset (the engine's layers, D-113), run and printed by
//! [`teigeodata_server`], as the servers do, with no bounds.
//!
//! The files are read into memory: the CLI does not own them (D-113). A query with no result
//! prints its empty output and exits 1.

use std::io::Write;
use std::path::{Path, PathBuf};

use teigeodata_engine::{Engine, Layer, LayerOptions, Load, Scratch};
use teigeodata_server::output::{self, Mode};
use teigeodata_server::query::{self as q, Limits};
pub(crate) use teigeodata_server::query::{Ask, PostalAsk};

use crate::Fail;

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

/// The packs given, or else the store's installed packs.
fn open(store: &Path, packs: &[PathBuf]) -> Result<Engine, Fail> {
    if packs.is_empty() {
        crate::sync::engine(&crate::sync::open(store)?, &[])
    } else {
        engine(packs)
    }
}

/// A point from degrees, on grid `G`.
pub(crate) fn point<G>(lat: f64, lon: f64) -> Result<teigeodata_core::Point<G>, Fail>
where
    G: teigeodata_core::Grid,
{
    Ok(q::point(lat, lon)?)
}

/// Runs `ask` with no bounds ([`Limits::NONE`]) and prints its answer in `mode`.
pub(crate) fn run(
    store: &Path,
    packs: &[PathBuf],
    ask: &Ask,
    mode: Mode,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let e = open(store, packs)?;
    let mut items = Vec::new();
    q::answer(&e, ask, &Limits::NONE, &mut Scratch::default(), &mut items)?;
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
    store: &Path,
    packs: &[PathBuf],
    (lat, lon): (f64, f64),
    json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let e = open(store, packs)?;
    let h = q::elevation(&e, lat, lon, &mut Scratch::default())?;
    output::elevation(lat, lon, h.as_ref(), json, out)?;
    match h {
        Some(_) => Ok(()),
        None => Err(Fail::Refused("no terrain there".into())),
    }
}

/// `tgd postal`: a code to its places, codes by prefix, or the codes at a point (D-200). One
/// line per code and place, or a JSON array; nothing found is exit 1.
pub(crate) fn postal(
    store: &Path,
    packs: &[PathBuf],
    ask: &PostalAsk,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let e = open(store, packs)?;
    let mut codes = Vec::new();
    q::postal(&e, ask, &Limits::NONE, &mut Scratch::default(), &mut codes)?;
    output::postal(&e, &codes, json, out)?;
    if codes.is_empty() {
        return Err(Fail::Refused("no postal code".into()));
    }
    Ok(())
}
