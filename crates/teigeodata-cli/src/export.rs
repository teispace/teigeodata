//! `export`: every place of a pack, streamed in its stored (Hilbert) order, as NDJSON or a
//! GeoJSON FeatureCollection, each place mapped by [`crate::output`]. One place is held at a
//! time; IDs and names are written straight from their buffers, so a row costs no allocation.
//! GeoJSON carries the pack's attribution as the foreign member `attribution`; NDJSON has
//! nowhere to put it, so it goes to standard error.

use std::io::Write;
use std::path::Path;

use teigeodata_engine::{Engine, Layer, LayerOptions, Load};

use crate::output::{self, Bufs, Item};
use crate::{Fail, Format};

pub(crate) fn run(path: &Path, format: Format, out: &mut dyn Write) -> Result<(), Fail> {
    let bad = |e: teigeodata_engine::Error| Fail::of(path.display(), e);
    let layer = Layer::open(
        path,
        LayerOptions {
            load: Load::Read,
            ..LayerOptions::default()
        },
    )
    .map_err(bad)?;
    let rows = u32::try_from(layer.rows()).map_err(|_| Fail::Refused("too many rows".into()))?;
    let mut engine = Engine::new();
    let id = engine.add(layer);
    let attribution = output::attribution(&engine);
    let geojson = format == Format::Geojson;
    if geojson {
        output::open_collection(out, attribution.as_deref())?;
    } else if let Some(a) = &attribution {
        let _ = writeln!(
            std::io::stderr(),
            "tgd: attribution: {}",
            a.replace('\n', "; ")
        );
    }
    let mut bufs = Bufs::default();
    for row in 0..rows {
        let Some(place) = engine.at_row(id, row).map_err(bad)? else {
            return Err(Fail::Refused(format!(
                "{}: row {row} has no place",
                path.display()
            )));
        };
        let record =
            output::record(&engine, &Item::from(place), &mut bufs, !geojson).map_err(bad)?;
        if geojson {
            if row > 0 {
                out.write_all(b",\n")?;
            }
            output::feature(out, &place, record)?;
        } else {
            serde_json::to_writer(&mut *out, &record)?;
            out.write_all(b"\n")?;
        }
    }
    if geojson {
        output::close_collection(out)?;
    }
    Ok(())
}
