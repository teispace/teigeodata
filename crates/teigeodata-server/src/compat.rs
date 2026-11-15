//! The Nominatim-compatible routes (ADR-0038, phase 6.2, D-314; research note
//! `nominatim-compat`): `/search`, `/reverse` and `/lookup` with the parameters and fields the
//! clients we checked use, mapped from the native answer by [`output::nominatim`]. What we cannot
//! fill is left out, never faked. No runtime: the fuzz harness runs this code as the server does.

use serde_json::{Map, Value, json};
use teigeodata_core::E7;
use teigeodata_engine::Engine;

use crate::body::WORK;
use crate::output::{self, Bufs, Item};
use crate::params::Params;
use crate::query::{self, Ask, Code, Limits, QueryError, point};
#[cfg(feature = "http")]
use crate::seams::Operation;

/// Nominatim's most results for one search (its `limit` "cannot be more than 40").
pub(crate) const MAX_LIMIT: usize = 40;

/// A Nominatim route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// `/search`.
    Search,
    /// `/reverse`.
    Reverse,
    /// `/lookup`.
    Lookup,
}

impl Route {
    /// The seams' operation.
    #[cfg(feature = "http")]
    pub(crate) const fn operation(self) -> Operation {
        match self {
            Self::Search => Operation::Search,
            Self::Reverse => Operation::Reverse,
            Self::Lookup => Operation::Lookup,
        }
    }
}

/// The output formats: `json` and `jsonv2` are the same for us (no `class`, no `place_rank`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Json,
    Geojson,
}

fn invalid(message: String) -> QueryError {
    QueryError::new(Code::InvalidArgument, message)
}

fn format(p: &Params) -> Result<Format, QueryError> {
    if p.get("json_callback").is_some() {
        return Err(invalid(
            "`json_callback` (JSONP) is not supported: call with CORS instead".into(),
        ));
    }
    match p.get("format") {
        None | Some("json" | "jsonv2") => Ok(Format::Json),
        Some("geojson") => Ok(Format::Geojson),
        Some(f) => Err(invalid(format!(
            "format {f:?} is not supported here: json, jsonv2 or geojson"
        ))),
    }
}

/// A `0`/`1` flag, with Nominatim's default.
fn flag(p: &Params, name: &str, default: bool) -> Result<bool, QueryError> {
    match p.get(name) {
        None => Ok(default),
        Some(_) => p.flag(name),
    }
}

/// A box from `x1,y1,x2,y2` (x is longitude), any two corners: (south, west, north, east).
fn viewbox(text: &str) -> Result<(f64, f64, f64, f64), QueryError> {
    let bad = || invalid(format!("`viewbox` is not x1,y1,x2,y2: {text:?}"));
    let mut v = [0.0_f64; 4];
    let mut parts = text.split(',');
    for x in &mut v {
        *x = parts
            .next()
            .and_then(|s| s.trim().parse().ok())
            .filter(|f: &f64| f.is_finite())
            .ok_or_else(bad)?;
    }
    if parts.next().is_some() {
        return Err(bad());
    }
    let [x1, y1, x2, y2] = v;
    let (south, north) = (y1.min(y2), y1.max(y2));
    let (west, east) = (x1.min(x2), x1.max(x2));
    if !(south < north && west < east) {
        return Err(bad());
    }
    point::<E7>(south, west)?;
    point::<E7>(north, east)?;
    Ok((south, west, north, east))
}

/// Metres per degree of latitude (WGS84 mean).
const M_PER_DEG: f64 = 111_320.0;

/// The search text: `q`, or the structured fields joined, most specific first.
fn search_text(p: &Params) -> Result<String, QueryError> {
    if let Some(q) = p.get("q") {
        return Ok(q.to_owned());
    }
    let parts: Vec<&str> = [
        "amenity",
        "street",
        "city",
        "county",
        "state",
        "country",
        "postalcode",
    ]
    .iter()
    .filter_map(|k| p.get(k))
    .filter(|v| !v.trim().is_empty())
    .collect();
    if parts.is_empty() {
        return Err(invalid("`q` or a structured field is missing".into()));
    }
    Ok(parts.join(", "))
}

/// The Nominatim objects of `items`, each with its `licence`.
fn objects(e: &Engine, items: &[Item]) -> Result<Vec<Value>, QueryError> {
    let licence = output::attribution_of(e, items).map(|a| a.replace('\n', "; "));
    let mut bufs = Bufs::default();
    items
        .iter()
        .map(|i| output::nominatim(e, i, licence.as_deref(), &mut bufs).map_err(QueryError::from))
        .collect()
}

fn lat_lon(o: &Value) -> Option<(f64, f64)> {
    Some((
        o.get("lat")?.as_str()?.parse().ok()?,
        o.get("lon")?.as_str()?.parse().ok()?,
    ))
}

/// One object as a GeoJSON feature: the properties without `lat`, `lon` and `boundingbox`,
/// which are the geometry and the `bbox`.
fn feature(mut o: Value) -> Value {
    let (lat, lon) = lat_lon(&o).unwrap_or_default();
    if let Some(m) = o.as_object_mut() {
        for k in ["lat", "lon", "boundingbox"] {
            m.remove(k);
        }
    }
    json!({
        "type": "Feature",
        "properties": o,
        "bbox": [lon, lat, lon, lat],
        "geometry": { "type": "Point", "coordinates": [lon, lat] },
    })
}

/// Answers Nominatim route `route` with parameters `p` within `limits` into `out`; the number
/// of places answered.
///
/// # Errors
/// A parameter not valid or past a bound, or the engine's.
pub(crate) fn answer(
    e: &Engine,
    route: Route,
    p: &Params,
    limits: &Limits,
    out: &mut Vec<u8>,
) -> Result<usize, QueryError> {
    let fmt = format(p)?;
    let mut objs = match route {
        Route::Search => search(e, p, limits)?,
        Route::Reverse => reverse(e, p, limits)?,
        Route::Lookup => lookup(e, p, limits)?,
    };
    let details = flag(p, "addressdetails", route == Route::Reverse)?;
    if !details {
        for o in &mut objs {
            if let Some(m) = o.as_object_mut() {
                m.remove("address");
            }
        }
    }
    let n = objs.len();
    let body = match (fmt, route) {
        (Format::Geojson, _) => {
            let mut fc = Map::new();
            fc.insert("type".into(), json!("FeatureCollection"));
            if let Some(l) = objs.first().and_then(|o| o.get("licence")).cloned() {
                fc.insert("licence".into(), l);
            }
            fc.insert(
                "features".into(),
                Value::Array(objs.into_iter().map(feature).collect()),
            );
            Value::Object(fc)
        }
        (Format::Json, Route::Reverse) => objs
            .into_iter()
            .next()
            .unwrap_or_else(|| json!({ "error": "Unable to geocode" })),
        (Format::Json, _) => Value::Array(objs),
    };
    serde_json::to_writer(&mut *out, &body)
        .map_err(|_| QueryError::new(Code::Internal, "the answer failed"))?;
    out.push(b'\n');
    Ok(n)
}

fn search(e: &Engine, p: &Params, limits: &Limits) -> Result<Vec<Value>, QueryError> {
    let limit = p.limit()?;
    let most = MAX_LIMIT.min(limits.limit);
    if limit > most {
        return Err(QueryError::new(
            Code::OutOfBounds,
            format!("limit {limit} is past the most allowed, {most}"),
        ));
    }
    let countries: Vec<String> = p
        .get("countrycodes")
        .map(|c| {
            c.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let bbox = p.get("viewbox").map(viewbox).transpose()?;
    let bounded = bbox.is_some() && flag(p, "bounded", false)?;
    let focus = bbox.map(|(s, w, n, east)| {
        let lat = f64::midpoint(s, n);
        let half_ns = (n - s) * M_PER_DEG / 2.0;
        let half_ew = (east - w) * M_PER_DEG * lat.to_radians().cos() / 2.0;
        (
            lat,
            f64::midpoint(w, east),
            half_ns.hypot(half_ew).max(1_000.0),
        )
    });
    // A filter answers fewer than it was given: ask for the most, then keep `limit`.
    let filtered = !countries.is_empty() || bounded;
    let ask = Ask::Search {
        query: search_text(p)?,
        limit: if filtered { limits.limit } else { limit },
        focus,
        strict: false,
    };
    WORK.with_borrow_mut(|(s, items, _)| {
        query::answer(e, &ask, limits, s, items)?;
        if bounded && let Some((south, w, n, east)) = bbox {
            items.retain(|i| {
                let (lat, lon) = (i.place.point.lat_degrees(), i.place.point.lon_degrees());
                (south..=n).contains(&lat) && (w..=east).contains(&lon)
            });
        }
        let mut objs = objects(e, items)?;
        if !countries.is_empty() {
            objs.retain(|o| {
                o.pointer("/address/country_code")
                    .and_then(Value::as_str)
                    .is_some_and(|c| countries.iter().any(|w| w == c))
            });
        }
        objs.truncate(limit);
        Ok(objs)
    })
}

fn reverse(e: &Engine, p: &Params, limits: &Limits) -> Result<Vec<Value>, QueryError> {
    let (lat, lon): (f64, f64) = (p.need("lat")?, p.need("lon")?);
    let zoom: u8 = p.parsed("zoom")?.unwrap_or(18);
    if zoom > 18 {
        return Err(invalid(format!("`zoom` is 0 to 18: {zoom}")));
    }
    WORK.with_borrow_mut(|(s, items, _)| {
        if zoom < 10 {
            // The deepest unit within the zoom's depth: 3 and below the country, 5 to 7 the
            // first level below it, 8 and 9 the second (research note §2).
            let depth = match zoom {
                0..=4 => 0,
                5..=7 => 1,
                _ => 2,
            };
            query::answer(e, &Ask::Admin { lat, lon }, limits, s, items)?;
            let deepest = items
                .iter()
                .rev()
                .find(|i| i.place.admin_level <= depth)
                .copied();
            items.clear();
            items.extend(deepest);
        } else {
            let ask = Ask::Reverse {
                lat,
                lon,
                limit: 1,
                in_country: true,
            };
            query::answer(e, &ask, limits, s, items)?;
            // The nearest place answers only near enough for the zoom: a point at sea or past
            // the packs' coverage is "Unable to geocode", not a place far away.
            let reach = reach_m(zoom);
            items.retain(|i| i.meters.is_some_and(|m| m <= reach));
        }
        objects(e, items)
    })
}

/// How far `/reverse` reaches for the nearest place at `zoom` (10 to 18), metres.
const fn reach_m(zoom: u8) -> f64 {
    match zoom {
        16.. => 25_000.0,
        13..=15 => 50_000.0,
        _ => 100_000.0,
    }
}

fn lookup(e: &Engine, p: &Params, limits: &Limits) -> Result<Vec<Value>, QueryError> {
    let raw = p.text("osm_ids")?;
    let entries: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if entries.len() > MAX_LOOKUP {
        return Err(QueryError::new(
            Code::OutOfBounds,
            format!("{} ids, at most {MAX_LOOKUP} are allowed", entries.len()),
        ));
    }
    // Our IDs are looked up; an OSM reference (`N123`) cannot be, and is left out as a place
    // not found is. Each entry is held to the text bound.
    let ask = Ask::Lookup {
        ids: entries
            .iter()
            .filter(|s| s.parse::<teigeodata_core::PlaceId>().is_ok())
            .map(|s| (*s).to_owned())
            .collect(),
    };
    let bounds = Limits {
        limit: MAX_LOOKUP,
        ..*limits
    };
    WORK.with_borrow_mut(|(s, items, _)| {
        query::answer(e, &ask, &bounds, s, items)?;
        objects(e, items)
    })
}

/// Nominatim's most IDs for one lookup.
pub(crate) const MAX_LOOKUP: usize = 50;

/// A failure as Nominatim answers it: `{"error": {"code": <status>, "message": …}}`.
#[cfg(feature = "http")]
pub(crate) fn error_body(e: &QueryError) -> Vec<u8> {
    let mut body = serde_json::to_vec(&json!({
        "error": { "code": e.code.http_status(), "message": e.message }
    }))
    .unwrap_or_default();
    body.push(b'\n');
    body
}
