//! The one mapping from a place to what `tgd` prints (ADR-0038): the native record, and the
//! GeoJSON and Nominatim forms made from it. A compatibility form never adds what the native
//! record lacks, and leaves out what it cannot fill (Nominatim's OSM `class`, `type`, `osm_id`).
//! The HTTP server (Phase 6) moves this module as it is.
//!
//! - **Native** (`--json`, NDJSON): `id`, `name`, `kind`, `admin_level`, `lat`, `lon`,
//!   `elevation` (m), `parent` (ID), `timezone` (its area's name), and for a query `meters`
//!   (distance) or `score` (search).
//! - **GeoJSON** (RFC 7946): a FeatureCollection of points, the native record as properties
//!   without `lat` and `lon` (they are the geometry), the attribution as a foreign member.
//! - **Nominatim** (`json` format): `place_id` (our stable ID), `licence` (the attribution),
//!   `lat` and `lon` as strings, `name`, `display_name` (the place and its ancestors),
//!   `boundingbox` (the point's: south, north, west, east, as strings), and `address`: each
//!   ancestor's name under `country` for a country and `admin_level_<n>` otherwise, and the
//!   ISO `country_code` from the ID's namespace.

use std::fmt::Display;
use std::io::Write;

use serde::{Serialize, Serializer};
use serde_json::{Map, Value, json};
use teigeodata_core::{PlaceId, PlaceKind};
use teigeodata_engine::{Engine, Error, Place};

use crate::Fail;

/// The longest name a buffer holds: NAMES caps a record far below it.
const NAME: usize = 1024;

/// A value serialized by its `Display`, with no allocation.
pub(crate) struct Shown<T>(pub(crate) T);

impl<T: Display> Serialize for Shown<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

/// The native record of one place.
#[derive(Serialize)]
pub(crate) struct Record<'a> {
    pub(crate) id: Shown<PlaceId>,
    pub(crate) name: Option<&'a str>,
    pub(crate) kind: Option<&'static str>,
    pub(crate) admin_level: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lon: Option<f64>,
    pub(crate) elevation: Option<i16>,
    pub(crate) parent: Option<Shown<PlaceId>>,
    pub(crate) timezone: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) meters: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) score: Option<i32>,
}

#[derive(Serialize)]
struct Geometry {
    r#type: &'static str,
    coordinates: [f64; 2],
}

#[derive(Serialize)]
struct Feature<'a> {
    r#type: &'static str,
    id: Shown<PlaceId>,
    geometry: Geometry,
    properties: Record<'a>,
}

/// Buffers for one record's names.
pub(crate) struct Bufs {
    name: [u8; NAME],
    tz: [u8; NAME],
}

impl Default for Bufs {
    fn default() -> Self {
        Self {
            name: [0; NAME],
            tz: [0; NAME],
        }
    }
}

/// One place a command found, with what the query adds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub(crate) place: Place,
    pub(crate) meters: Option<f64>,
    pub(crate) score: Option<i32>,
}

impl From<Place> for Item {
    fn from(place: Place) -> Self {
        Self {
            place,
            meters: None,
            score: None,
        }
    }
}

/// The native record of `item`; `with_point` puts `lat` and `lon` in it.
pub(crate) fn record<'b>(
    engine: &Engine,
    item: &Item,
    bufs: &'b mut Bufs,
    with_point: bool,
) -> Result<Record<'b>, Error> {
    let place = &item.place;
    let parent = engine.parent(place)?.map(|p| Shown(p.id));
    let timezone = match engine.tz_of(place)? {
        Some(z) => engine.name(&z, &mut bufs.tz)?,
        None => None,
    };
    Ok(Record {
        id: Shown(place.id),
        name: engine.name(place, &mut bufs.name)?,
        kind: place.kind.map(PlaceKind::name),
        admin_level: place.admin_level,
        lat: with_point.then(|| place.point.lat_degrees()),
        lon: with_point.then(|| place.point.lon_degrees()),
        elevation: engine.elevation(place)?,
        parent,
        timezone,
        meters: item.meters.map(|m| (m * 10.0).round() / 10.0),
        score: item.score,
    })
}

/// Writes `record` of `place` as a GeoJSON feature.
pub(crate) fn feature(
    out: &mut dyn Write,
    place: &Place,
    properties: Record<'_>,
) -> Result<(), Fail> {
    let f = Feature {
        r#type: "Feature",
        id: Shown(place.id),
        geometry: Geometry {
            r#type: "Point",
            coordinates: [place.point.lon_degrees(), place.point.lat_degrees()],
        },
        properties,
    };
    serde_json::to_writer(out, &f)?;
    Ok(())
}

/// Opens a FeatureCollection; [`close_collection`] ends it.
pub(crate) fn open_collection(out: &mut dyn Write, attribution: Option<&str>) -> Result<(), Fail> {
    out.write_all(b"{\"type\":\"FeatureCollection\",")?;
    if let Some(a) = attribution {
        out.write_all(b"\"attribution\":")?;
        serde_json::to_writer(&mut *out, a)?;
        out.write_all(b",")?;
    }
    out.write_all(b"\"features\":[\n")?;
    Ok(())
}

pub(crate) fn close_collection(out: &mut dyn Write) -> Result<(), Fail> {
    out.write_all(b"\n]}\n")?;
    Ok(())
}

/// How a query's results are printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Text,
    Json,
    Geojson,
    Nominatim,
}

/// The attribution of every layer, one notice per line, each once.
pub(crate) fn attribution(engine: &Engine) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    for l in engine.layers() {
        for line in l.attribution().unwrap_or_default().lines() {
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Writes `items` in `mode`.
pub(crate) fn write(
    engine: &Engine,
    items: &[Item],
    mode: Mode,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let bad = |e: Error| Fail::Refused(e.to_string());
    let mut bufs = Bufs::default();
    match mode {
        Mode::Text => {
            for item in items {
                let r = record(engine, item, &mut bufs, true).map_err(bad)?;
                text(out, &r)?;
            }
        }
        Mode::Json => {
            out.write_all(b"[")?;
            for (i, item) in items.iter().enumerate() {
                out.write_all(if i == 0 { b"\n" } else { b",\n" })?;
                let r = record(engine, item, &mut bufs, true).map_err(bad)?;
                serde_json::to_writer(&mut *out, &r)?;
            }
            out.write_all(b"\n]\n")?;
        }
        Mode::Geojson => {
            open_collection(out, attribution(engine).as_deref())?;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.write_all(b",\n")?;
                }
                let r = record(engine, item, &mut bufs, false).map_err(bad)?;
                feature(out, &item.place, r)?;
            }
            close_collection(out)?;
        }
        Mode::Nominatim => {
            let licence = attribution(engine).map(|a| a.replace('\n', "; "));
            let v: Vec<Value> = items
                .iter()
                .map(|item| nominatim(engine, item, licence.as_deref(), &mut bufs))
                .collect::<Result<_, _>>()
                .map_err(bad)?;
            serde_json::to_writer_pretty(&mut *out, &v)?;
            writeln!(out)?;
        }
    }
    Ok(())
}

fn text(out: &mut dyn Write, r: &Record<'_>) -> Result<(), Fail> {
    write!(
        out,
        "{}  {}",
        r.name.unwrap_or("(no name)"),
        r.kind.unwrap_or("?")
    )?;
    if r.admin_level > 0 {
        write!(out, " {}", r.admin_level)?;
    }
    write!(
        out,
        "  {:.6},{:.6}  {}",
        r.lat.unwrap_or_default(),
        r.lon.unwrap_or_default(),
        r.id.0
    )?;
    if let Some(e) = r.elevation {
        write!(out, "  {e} m")?;
    }
    if let Some(t) = r.timezone {
        write!(out, "  {t}")?;
    }
    if let Some(m) = r.meters {
        write!(out, "  {m} m away")?;
    }
    if let Some(s) = r.score {
        write!(out, "  score {s}")?;
    }
    writeln!(out)?;
    Ok(())
}

/// The Nominatim `json` object of `item`, from its native record.
fn nominatim(
    engine: &Engine,
    item: &Item,
    licence: Option<&str>,
    bufs: &mut Bufs,
) -> Result<Value, Error> {
    let place = &item.place;
    let mut ancestors = Vec::new();
    engine.ancestors(place, &mut ancestors)?;
    let mut address = Map::new();
    let mut display = Vec::new();
    let mut buf = [0_u8; NAME];
    for a in core::iter::once(place).chain(&ancestors) {
        let Some(n) = engine.name(a, &mut buf)? else {
            continue;
        };
        display.push(n.to_owned());
        if a.id == place.id {
            continue;
        }
        let key = if a.kind == Some(PlaceKind::Country) {
            "country".to_string()
        } else {
            format!("admin_level_{}", a.admin_level)
        };
        address.entry(key).or_insert_with(|| json!(n));
    }
    if let Some(cc) = place.id.namespace().country() {
        let cc: String = cc
            .iter()
            .map(|b| char::from(*b).to_ascii_lowercase())
            .collect();
        address.insert("country_code".into(), json!(cc));
    }
    let r = record(engine, item, bufs, true)?;
    let (lat, lon) = (place.point.lat_degrees(), place.point.lon_degrees());
    let mut v = json!({
        "place_id": place.id.to_string(),
        "lat": lat.to_string(),
        "lon": lon.to_string(),
        "display_name": display.join(", "),
        "boundingbox": [lat.to_string(), lat.to_string(), lon.to_string(), lon.to_string()],
        "address": address,
    });
    if let Some(o) = v.as_object_mut() {
        if let Some(l) = licence {
            o.insert("licence".into(), json!(l));
        }
        if let Some(n) = r.name {
            o.insert("name".into(), json!(n));
        }
    }
    Ok(v)
}
