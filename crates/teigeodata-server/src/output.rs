//! The one mapping from a place to what `tgd` prints (ADR-0038): the native record, and the
//! GeoJSON and Nominatim forms made from it. A compatibility form never adds what the native
//! record lacks, and leaves out what it cannot fill (Nominatim's OSM `class`, `type`, `osm_id`).
//!
//! - **Native** (`--json`, NDJSON): `id`, `name`, `kind`, `admin_level`, `lat`, `lon`,
//!   `elevation` (m), `elevation_source` (`surveyed` or `model`, when stated), `parent` (ID), `timezone` (its area's name), `iso_code` (ISO 3166, a
//!   country's or unit's, only when it has one, D-197), and for a query `meters` (distance),
//!   `bearing` (degrees clockwise from north, from the place to the point asked about) and
//!   `inside` (whether the point is in a country's or unit's outline; `reverse`, TG-19), or
//!   `score` (search).
//! - **GeoJSON** (RFC 7946): a FeatureCollection of points, the native record as properties
//!   without `lat` and `lon` (they are the geometry), the attribution of the packs the places
//!   came from as a foreign member.
//! - **Nominatim** (`json` format): `place_id` (our stable ID), `licence` (the same attribution),
//!   `lat` and `lon` as strings, `name`, `display_name` (the place and its ancestors),
//!   `boundingbox` (the point's: south, north, west, east, as strings), and `address`: each
//!   ancestor's name under `country` for a country and `admin_level_<n>` otherwise, and the
//!   ISO `country_code`: the country's ISO 3166 code where the packs have it, else the ID's
//!   namespace.

use std::fmt::{self, Display};
use std::io::{self, Write};

use serde::{Serialize, Serializer};
use serde_json::{Map, Value, json};
use teigeodata_core::{IsoCode, PlaceId, PlaceKind};
use teigeodata_engine::{Engine, Error, Height, HeightSource, Layer, LayerId, Place, PostalCode};

/// Why writing an answer failed.
#[derive(Debug)]
pub enum OutputError {
    /// The writer failed (a closed pipe is [`io::ErrorKind::BrokenPipe`]).
    Io(io::Error),
    /// A query the answer needs failed.
    Engine(Error),
}

impl Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Engine(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Engine(e) => Some(e),
        }
    }
}

impl From<io::Error> for OutputError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<Error> for OutputError {
    fn from(e: Error) -> Self {
        Self::Engine(e)
    }
}

impl From<serde_json::Error> for OutputError {
    fn from(e: serde_json::Error) -> Self {
        // Serializing our records cannot fail but by the writer.
        Self::Io(e.into())
    }
}

/// The longest name a buffer holds: NAMES caps a record far below it.
pub const NAME: usize = 1024;

/// A value serialized by its `Display`, with no allocation.
#[derive(Clone, Copy, Debug)]
pub struct Shown<T>(pub T);

impl<T: Display> Serialize for Shown<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}

/// The native record of one place.
#[derive(Debug, Serialize)]
pub struct Record<'a> {
    /// Its stable ID.
    pub id: Shown<PlaceId>,
    /// Its canonical name.
    pub name: Option<&'a str>,
    /// Its kind (`city`, `country`, ...).
    pub kind: Option<&'static str>,
    /// Its admin level: 0 for a point, 1 for a country, deeper units after.
    pub admin_level: u8,
    /// Latitude, WGS84 degrees (left out of a GeoJSON feature's properties).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    /// Longitude, WGS84 degrees (as `lat`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    /// Height above the geoid, metres.
    pub elevation: Option<i16>,
    /// Where the height came from: `surveyed` or `model` (D-219).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_source: Option<&'static str>,
    /// Its parent's ID.
    pub parent: Option<Shown<PlaceId>>,
    /// Its timezone area's name (`Asia/Kathmandu`).
    pub timezone: Option<&'a str>,
    /// Its ISO 3166 code, when it has one (D-197).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iso_code: Option<Shown<IsoCode>>,
    /// Distance from the point asked about, metres (one decimal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meters: Option<f64>,
    /// Bearing from the place to the point asked about, degrees clockwise from north.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<f64>,
    /// Whether the point asked about is inside the place's outline (`reverse`, TG-19).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inside: Option<bool>,
    /// Its search score, comparable only within one query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i32>,
    /// Every name, canonical first, with its language and origin (`tgd get`, D-227).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<NameOut>>,
    /// The UTC offset at the instant asked: `+05:45` (`tgd tz`, TG-03).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_offset: Option<String>,
    /// The zone's abbreviation at that instant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tz_abbr: Option<String>,
    /// Whether daylight saving time is in force at that instant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_dst: Option<bool>,
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
#[derive(Debug)]
pub struct Bufs {
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
pub struct Item {
    /// The place.
    pub place: Place,
    /// Distance from the point asked about, metres.
    pub meters: Option<f64>,
    /// Bearing from the place to the point asked about, degrees clockwise from north.
    pub bearing: Option<f64>,
    /// Whether the point asked about is inside the place's outline.
    pub inside: Option<bool>,
    /// Its search score.
    pub score: Option<i32>,
    /// A timezone area's UTC offset at the instant asked (`tgd tz`).
    pub offset: Option<teigeodata_engine::Offset>,
    /// List every name with its language and origin (`tgd get`, D-227).
    pub all_names: bool,
}

impl From<Place> for Item {
    fn from(place: Place) -> Self {
        Self {
            place,
            meters: None,
            bearing: None,
            inside: None,
            score: None,
            offset: None,
            all_names: false,
        }
    }
}

/// An optional field: absent when its component is not installed (D-133), so a store that
/// installed part of a pack answers with what it has; any other error stands.
///
/// # Errors
/// Any error of `r` but [`Error::MissingComponent`].
pub fn installed<T>(r: Result<Option<T>, Error>) -> Result<Option<T>, Error> {
    match r {
        Err(Error::MissingComponent { .. }) => Ok(None),
        r => r,
    }
}

/// One name of a place: its text, language tag (`ne-Deva`), type and origin.
#[derive(Debug, Serialize)]
pub struct NameOut {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    origin: &'static str,
    machine: bool,
}

/// Every name of `place` (D-227).
fn all_names(engine: &Engine, place: &Place, buf: &mut [u8]) -> Result<Vec<NameOut>, Error> {
    let mut out = Vec::new();
    engine.names(place, buf, |text, info| {
        let tag = |t: teigeodata_engine::Tag| {
            let (b, n) = t.to_bytes();
            String::from_utf8_lossy(b.get(..n).unwrap_or_default()).into_owned()
        };
        let lang = match (info.lang, info.script) {
            (Some(l), Some(s)) => Some(format!("{}-{}", tag(l), tag(s))),
            (Some(l), None) => Some(tag(l)),
            (None, Some(s)) => Some(format!("und-{}", tag(s))),
            (None, None) => None,
        };
        out.push(NameOut {
            text: text.to_owned(),
            lang,
            kind: info.kind.map(teigeodata_engine::NameType::as_str),
            origin: info.origin.name(),
            machine: info.origin.is_machine(),
        });
    })?;
    Ok(out)
}

/// The native record of `item`; `with_point` puts `lat` and `lon` in it.
///
/// # Errors
/// A query the record needs failed (a component not installed leaves its field out).
pub fn record<'b>(
    engine: &Engine,
    item: &Item,
    bufs: &'b mut Bufs,
    with_point: bool,
) -> Result<Record<'b>, Error> {
    let place = &item.place;
    let parent = installed(engine.parent(place))?.map(|p| Shown(p.id));
    let timezone = match installed(engine.tz_of(place))? {
        Some(z) => installed(engine.name(&z, &mut bufs.tz))?,
        None => None,
    };
    Ok(Record {
        id: Shown(place.id),
        name: installed(engine.name(place, &mut bufs.name))?,
        kind: place.kind.map(PlaceKind::name),
        admin_level: place.admin_level,
        lat: with_point.then(|| place.point.lat_degrees()),
        lon: with_point.then(|| place.point.lon_degrees()),
        elevation: installed(engine.elevation(place))?,
        elevation_source: installed(engine.elevation_source(place))?.map(HeightSource::name),
        parent,
        timezone,
        iso_code: installed(engine.iso_code(place))?.map(Shown),
        meters: item.meters.map(|m| (m * 10.0).round() / 10.0),
        bearing: item.bearing.map(|b| (b * 10.0).round() / 10.0),
        inside: item.inside,
        score: item.score,
        names: if item.all_names {
            installed(all_names(engine, place, &mut [0; NAME]).map(Some))?
        } else {
            None
        },
        utc_offset: item.offset.map(|o| hhmm(o.seconds)),
        tz_abbr: item.offset.as_ref().map(|o| o.abbr().to_owned()),
        is_dst: item.offset.map(|o| o.is_dst),
    })
}

/// Writes `record` of `place` as a GeoJSON feature.
///
/// # Errors
/// The writer failed.
pub fn feature(
    out: &mut dyn Write,
    place: &Place,
    properties: Record<'_>,
) -> Result<(), OutputError> {
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

/// Opens a FeatureCollection, with `attribution` as a foreign member; [`close_collection`]
/// ends it.
///
/// # Errors
/// The writer failed.
pub fn open_collection(out: &mut dyn Write, attribution: Option<&str>) -> Result<(), OutputError> {
    out.write_all(b"{\"type\":\"FeatureCollection\",")?;
    if let Some(a) = attribution {
        out.write_all(b"\"attribution\":")?;
        serde_json::to_writer(&mut *out, a)?;
        out.write_all(b",")?;
    }
    out.write_all(b"\"features\":[\n")?;
    Ok(())
}

/// Ends a FeatureCollection [`open_collection`] began.
///
/// # Errors
/// The writer failed.
pub fn close_collection(out: &mut dyn Write) -> Result<(), OutputError> {
    out.write_all(b"\n]}\n")?;
    Ok(())
}

/// How a query's results are printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One line a place, for a terminal.
    Text,
    /// The native records, a JSON array.
    Json,
    /// A GeoJSON FeatureCollection (RFC 7946).
    Geojson,
    /// Nominatim's `json` format.
    Nominatim,
}

/// The attribution of every layer, one notice per line, each once: for a whole extract
/// (`tgd export`).
#[must_use]
pub fn attribution(engine: &Engine) -> Option<String> {
    notices(engine.layers())
}

/// The attribution of the layers `items` came from, one notice per line, each once (D-032):
/// what an answer carries. `None` for an answer with no place, or from layers with no notices.
#[must_use]
pub fn attribution_of(engine: &Engine, items: &[Item]) -> Option<String> {
    let mut ids: Vec<LayerId> = items.iter().map(|i| i.place.layer).collect();
    ids.sort_unstable();
    ids.dedup();
    notices(ids.into_iter().filter_map(|id| engine.layer(id)))
}

fn notices<'a>(layers: impl Iterator<Item = &'a Layer>) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    for l in layers {
        for line in l.attribution().unwrap_or_default().lines() {
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Writes `items` in `mode`.
///
/// # Errors
/// A query a record needs failed, or the writer failed.
pub fn write(
    engine: &Engine,
    items: &[Item],
    mode: Mode,
    out: &mut dyn Write,
) -> Result<(), OutputError> {
    let mut bufs = Bufs::default();
    match mode {
        Mode::Text => {
            for item in items {
                let r = record(engine, item, &mut bufs, true)?;
                text(out, &r)?;
            }
        }
        Mode::Json => {
            out.write_all(b"[")?;
            for (i, item) in items.iter().enumerate() {
                out.write_all(if i == 0 { b"\n" } else { b",\n" })?;
                let r = record(engine, item, &mut bufs, true)?;
                serde_json::to_writer(&mut *out, &r)?;
            }
            out.write_all(b"\n]\n")?;
        }
        Mode::Geojson => {
            open_collection(out, attribution_of(engine, items).as_deref())?;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.write_all(b",\n")?;
                }
                let r = record(engine, item, &mut bufs, false)?;
                feature(out, &item.place, r)?;
            }
            close_collection(out)?;
        }
        Mode::Nominatim => {
            let licence = attribution_of(engine, items).map(|a| a.replace('\n', "; "));
            let v: Vec<Value> = items
                .iter()
                .map(|item| nominatim(engine, item, licence.as_deref(), &mut bufs))
                .collect::<Result<_, _>>()?;
            serde_json::to_writer_pretty(&mut *out, &v)?;
            writeln!(out)?;
        }
    }
    Ok(())
}

fn text(out: &mut dyn Write, r: &Record<'_>) -> Result<(), OutputError> {
    write!(
        out,
        "{}  {}",
        r.name.unwrap_or("(no name)"),
        r.kind.unwrap_or("?")
    )?;
    if r.admin_level > 0 {
        write!(out, " {}", r.admin_level)?;
    }
    if let Some(c) = &r.iso_code {
        write!(out, " {}", c.0)?;
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
        if let Some(s) = r.elevation_source {
            write!(out, " ({s})")?;
        }
    }
    if let Some(t) = r.timezone {
        write!(out, "  {t}")?;
    }
    if let Some(m) = r.meters {
        write!(out, "  {m} m away")?;
        if let Some(b) = r.bearing.filter(|_| m > 0.0) {
            write!(out, " at {b}°")?;
        }
    }
    if r.inside == Some(true) {
        write!(out, "  inside")?;
    }
    if let Some(s) = r.score {
        write!(out, "  score {s}")?;
    }
    if let Some(o) = &r.utc_offset {
        write!(out, "  UTC{o}")?;
        if let Some(a) = &r.tz_abbr {
            write!(
                out,
                " ({a}{})",
                if r.is_dst == Some(true) { ", DST" } else { "" }
            )?;
        }
    }
    writeln!(out)?;
    for n in r.names.iter().flatten() {
        writeln!(
            out,
            "  {}  {}  {}{}",
            n.lang.as_deref().unwrap_or("-"),
            n.text,
            n.origin,
            if n.machine { ", machine" } else { "" }
        )?;
    }
    Ok(())
}

/// `+05:45` for 20,700 seconds east of UTC.
fn hhmm(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let s = seconds.unsigned_abs();
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    if sec == 0 {
        format!("{sign}{h:02}:{m:02}")
    } else {
        format!("{sign}{h:02}:{m:02}:{sec:02}")
    }
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
    installed(engine.ancestors(place, &mut ancestors).map(Some))?;
    let mut address = Map::new();
    let mut display = Vec::new();
    let mut buf = [0_u8; NAME];
    let mut country_code = None;
    for a in core::iter::once(place).chain(&ancestors) {
        if a.kind == Some(PlaceKind::Country) && country_code.is_none() {
            country_code = installed(engine.iso_code(a))?.map(IsoCode::country);
        }
        let Some(n) = installed(engine.name(a, &mut buf))? else {
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
    if let Some(cc) = country_code.or_else(|| place.id.namespace().country()) {
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

/// Writes a terrain height at `lat`, `lon` (from [`crate::query::elevation`]): one line for a
/// terminal, or the native JSON object; with no terrain there, nothing, or the object with
/// `elevation_m` null.
///
/// # Errors
/// The writer failed.
pub fn elevation(
    lat: f64,
    lon: f64,
    h: Option<&Height>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), OutputError> {
    match (h, json) {
        (Some(h), false) => {
            let from = h.source.map_or("an unstated DEM", |s| s.name());
            writeln!(
                out,
                "{:.1} m above {} ({from}, {:.0} m spacing)",
                h.meters,
                h.datum.name(),
                h.resolution_m
            )?;
        }
        (Some(h), true) => writeln!(
            out,
            "{{\"lat\":{lat},\"lon\":{lon},\"elevation_m\":{:.1},\"datum\":\"{}\",\"source\":{},\"resolution_m\":{:.1}}}",
            h.meters,
            h.datum.name(),
            h.source
                .map_or_else(|| "null".to_owned(), |s| format!("\"{}\"", s.key())),
            h.resolution_m
        )?,
        (None, true) => writeln!(
            out,
            "{{\"lat\":{lat},\"lon\":{lon},\"elevation_m\":null,\"datum\":\"EGM2008\"}}"
        )?,
        (None, false) => {}
    }
    Ok(())
}

/// Writes postal codes (from [`crate::query::postal`]): one line per code and place, or a
/// JSON array of `scheme`, `code`, `kind`, `valid_from`, `valid_to`, `replaced`, `id`, `name`
/// and `admin_level`.
///
/// # Errors
/// A name lookup failed, or the writer failed.
pub fn postal(
    engine: &Engine,
    codes: &[PostalCode],
    json: bool,
    out: &mut dyn Write,
) -> Result<(), OutputError> {
    let mut buf = [0_u8; NAME];
    let mut rows = Vec::with_capacity(codes.len());
    for c in codes {
        let name = installed(engine.name(&c.place, &mut buf))?.map(str::to_owned);
        rows.push(json!({
            "scheme": c.scheme(),
            "code": c.code(),
            "kind": crate::query::code_kind(c.kind),
            "valid_from": c.valid_from,
            "valid_to": c.valid_to,
            "replaced": c.is_replaced(),
            "id": c.place.id.to_string(),
            "name": name,
            "admin_level": c.place.admin_level,
        }));
    }
    if json {
        writeln!(out, "{}", Value::Array(rows))?;
    } else {
        for (c, r) in codes.iter().zip(&rows) {
            writeln!(
                out,
                "{} {}\t{}\t{}\t{}{}",
                c.scheme(),
                c.code(),
                crate::query::code_kind(c.kind),
                c.place.id,
                r["name"].as_str().unwrap_or("-"),
                if c.is_replaced() { "\t(replaced)" } else { "" }
            )?;
        }
    }
    Ok(())
}
