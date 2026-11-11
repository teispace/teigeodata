//! `export`: every place of a pack, streamed in its stored (Hilbert) order, as NDJSON or a
//! GeoJSON FeatureCollection (RFC 7946). One place is held at a time; IDs and names are
//! written straight from their buffers, so a row costs no allocation.
//!
//! A record: `id`, `name` (canonical), `kind`, `admin_level`, `lat`, `lon`, `elevation` (m),
//! `parent` (ID), `timezone` (the name of its timezone area). A field the pack does not hold is
//! `null`. GeoJSON carries the pack's attribution as the foreign member `attribution`; NDJSON
//! has nowhere to put it, so it goes to standard error.

use std::fmt::Display;
use std::io::Write;
use std::path::Path;

use serde::{Serialize, Serializer};
use teigeodata_engine::{Engine, Layer, LayerOptions, Load, Place, Verify};

use crate::{Fail, Format};

/// A value serialized by its `Display`, with no allocation.
struct Shown<T>(T);

impl<T: Display> Serialize for Shown<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

/// The properties of one place: the one mapping NDJSON and GeoJSON share.
#[derive(Serialize)]
struct Record<'a> {
    id: Shown<teigeodata_core::PlaceId>,
    name: Option<&'a str>,
    kind: Option<&'static str>,
    admin_level: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lon: Option<f64>,
    elevation: Option<i16>,
    parent: Option<Shown<teigeodata_core::PlaceId>>,
    timezone: Option<&'a str>,
}

#[derive(Serialize)]
struct Point {
    r#type: &'static str,
    coordinates: [f64; 2],
}

#[derive(Serialize)]
struct Feature<'a> {
    r#type: &'static str,
    id: Shown<teigeodata_core::PlaceId>,
    geometry: Point,
    properties: Record<'a>,
}

/// A failed query, named by the pack.
fn bad(path: &Path) -> impl Fn(teigeodata_engine::Error) -> Fail + '_ {
    move |e| Fail::of(path.display(), e)
}

pub(crate) fn run(path: &Path, format: Format, out: &mut dyn Write) -> Result<(), Fail> {
    let layer = Layer::open(
        path,
        LayerOptions {
            load: Load::Read,
            verify: Verify::Lazy,
            ..LayerOptions::default()
        },
    )
    .map_err(bad(path))?;
    let rows = u32::try_from(layer.rows()).map_err(|_| Fail::Refused("too many rows".into()))?;
    let attribution = layer.attribution().map(str::to_owned);
    let mut engine = Engine::new();
    let id = engine.add(layer);
    let (mut name_buf, mut tz_buf) = ([0_u8; 1024], [0_u8; 1024]);
    let geojson = format == Format::Geojson;
    if geojson {
        out.write_all(b"{\"type\":\"FeatureCollection\",")?;
        if let Some(a) = &attribution {
            out.write_all(b"\"attribution\":")?;
            serde_json::to_writer(&mut *out, a)?;
            out.write_all(b",")?;
        }
        out.write_all(b"\"features\":[\n")?;
    } else if let Some(a) = &attribution {
        let _ = writeln!(
            std::io::stderr(),
            "tgd: attribution: {}",
            a.replace('\n', "; ")
        );
    }
    let mut first = true;
    for row in 0..rows {
        let Some(place) = engine.at_row(id, row).map_err(bad(path))? else {
            return Err(Fail::Refused(format!(
                "{}: row {row} has no place",
                path.display()
            )));
        };
        let record =
            record(&engine, &place, &mut name_buf, &mut tz_buf, !geojson).map_err(bad(path))?;
        if geojson {
            if !first {
                out.write_all(b",\n")?;
            }
            let f = Feature {
                r#type: "Feature",
                id: Shown(place.id),
                geometry: Point {
                    r#type: "Point",
                    coordinates: [place.point.lon_degrees(), place.point.lat_degrees()],
                },
                properties: record,
            };
            serde_json::to_writer(&mut *out, &f)?;
        } else {
            serde_json::to_writer(&mut *out, &record)?;
            out.write_all(b"\n")?;
        }
        first = false;
    }
    if geojson {
        out.write_all(b"\n]}\n")?;
    }
    Ok(())
}

/// The record of `place`; `with_point` puts `lat` and `lon` in it (GeoJSON has them in the
/// geometry).
fn record<'b>(
    engine: &Engine,
    place: &Place,
    name_buf: &'b mut [u8],
    tz_buf: &'b mut [u8],
    with_point: bool,
) -> Result<Record<'b>, teigeodata_engine::Error> {
    let parent = engine.parent(place)?.map(|p| Shown(p.id));
    let timezone = match engine.tz_of(place)? {
        Some(z) => engine.name(&z, tz_buf)?,
        None => None,
    };
    Ok(Record {
        id: Shown(place.id),
        name: engine.name(place, name_buf)?,
        kind: place.kind.map(teigeodata_core::PlaceKind::name),
        admin_level: place.admin_level,
        lat: with_point.then(|| place.point.lat_degrees()),
        lon: with_point.then(|| place.point.lon_degrees()),
        elevation: engine.elevation(place)?,
        parent,
        timezone,
    })
}
