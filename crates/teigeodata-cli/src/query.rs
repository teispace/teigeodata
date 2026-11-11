//! The queries: `get`, `reverse`, `near`, `search`, `admin` and `tz`, over the packs given with
//! `--pack` as one dataset (the engine's layers, D-113), printed by [`crate::output`].
//!
//! The files are read into memory: the CLI does not own them (D-113). A query with no result
//! prints its empty output and exits 1.

use std::io::Write;
use std::path::PathBuf;

use teigeodata_core::{E6, E7, PlaceId, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Load, Place, Scratch};

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

/// `tgd search`'s ranking: a focus (latitude, longitude, scale in metres) and strict order.
fn search_options(
    (focus, strict): (Option<(f64, f64, f64)>, bool),
) -> Result<teigeodata_engine::SearchOptions, Fail> {
    let opts = teigeodata_engine::SearchOptions::default().with_strict(strict);
    let Some((lat, lon, scale_m)) = focus else {
        return Ok(opts);
    };
    if !(scale_m.is_finite() && scale_m > 0.0) {
        return Err(Fail::Refused(format!(
            "--scale-km must be positive: {scale_m} m"
        )));
    }
    let mut f = teigeodata_engine::Focus::new(point(lat, lon)?);
    f.scale_m = scale_m;
    Ok(opts.with_focus(f))
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
        /// A focus (latitude, longitude, scale in metres) and strict order.
        rank: (Option<(f64, f64, f64)>, bool),
    },
    Admin {
        lat: f64,
        lon: f64,
    },
    TzAt {
        lat: f64,
        lon: f64,
        t: i64,
    },
    TzOf {
        id: String,
        t: i64,
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
            let at = point::<E6>(lat, lon)?;
            e.reverse(at, k, &mut s, &mut near)?;
            e.mark_inside(at, &mut s, &mut near)?;
            near.iter()
                .map(|n| Item {
                    place: n.place,
                    meters: Some(n.meters),
                    bearing: Some(n.bearing_deg),
                    inside: n.inside,
                    score: None,
                    offset: None,
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
                    bearing: Some(n.bearing_deg),
                    inside: None,
                    score: None,
                    offset: None,
                })
                .collect()
        }
        Ask::Search { query, k, rank } => {
            let opts = search_options(rank)?;
            let mut hits = Vec::new();
            e.search_with(&query, k, &opts, &mut s, &mut hits)?;
            hits.iter()
                .map(|h| Item {
                    place: h.place,
                    meters: None,
                    bearing: None,
                    inside: None,
                    score: Some(h.score),
                    offset: None,
                })
                .collect()
        }
        Ask::Admin { lat, lon } => {
            let mut units = Vec::new();
            e.admin_at(point::<E7>(lat, lon)?, &mut s, &mut units)?;
            units.into_iter().map(Item::from).collect()
        }
        Ask::TzAt { lat, lon, t } => {
            let area = e.tz_at(point::<E7>(lat, lon)?)?;
            with_offsets(&e, area, t)?
        }
        Ask::TzOf { id: text, t } => {
            let place = e
                .get(id(&text)?)?
                .ok_or_else(|| Fail::Refused(format!("{text}: not found")))?;
            let area = e.tz_of(&place)?;
            with_offsets(&e, area, t)?
        }
    };
    output::write(&e, &items, mode, out)?;
    // As grep: no result is exit 1, after the (empty) output a script may still parse.
    if items.is_empty() {
        return Err(Fail::Refused("no result".into()));
    }
    Ok(())
}

/// A timezone area with the UTC offset in force at `t` when its rules are installed (TG-03).
fn with_offsets(e: &Engine, area: Option<Place>, t: i64) -> Result<Vec<Item>, Fail> {
    let mut out = Vec::new();
    if let Some(a) = area {
        let mut item = Item::from(a);
        if let Some(zone) = e.zone_name(&a)?
            && let Some(o) = output::installed(e.tz_offset_at(zone, t))?
        {
            item.offset = Some(o);
        }
        out.push(item);
    }
    Ok(out)
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
    match (&h, json) {
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
    match h {
        Some(_) => Ok(()),
        None => Err(Fail::Refused("no terrain there".into())),
    }
}

/// What `tgd postal` asks.
#[derive(Debug)]
pub(crate) enum PostalAsk {
    /// A code to its places; with `prefix`, up to `limit` codes starting with it.
    Code {
        code: String,
        scheme: Option<String>,
        prefix: bool,
        limit: usize,
    },
    /// The codes at a point (D-200).
    At { lat: f64, lon: f64 },
}

const fn code_kind(kind: teigeodata_engine::CodeKind) -> &'static str {
    use teigeodata_engine::CodeKind;
    match kind {
        CodeKind::Area => "area",
        CodeKind::Part => "part",
        CodeKind::PostOffice => "post_office",
        CodeKind::Point => "point",
    }
}

/// `tgd postal`: a code to its places, codes by prefix, or the codes at a point (D-200). One
/// line per code and place, or a JSON array; nothing found is exit 1.
pub(crate) fn postal(
    store: &std::path::Path,
    packs: &[PathBuf],
    ask: PostalAsk,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let e = if packs.is_empty() {
        crate::sync::Opened::open(store)?.engine(&[])?
    } else {
        engine(packs)?
    };
    let mut codes = Vec::new();
    match ask {
        PostalAsk::Code {
            code,
            scheme,
            prefix: false,
            ..
        } => e.postal_lookup(&code, scheme.as_deref(), &mut codes)?,
        PostalAsk::Code {
            code,
            scheme,
            prefix: true,
            limit,
        } => e.postal_prefix(&code, scheme.as_deref(), limit, &mut codes)?,
        PostalAsk::At { lat, lon } => {
            e.postal_at(point::<E7>(lat, lon)?, &mut Scratch::default(), &mut codes)?;
        }
    }
    let mut buf = [0_u8; 1024];
    let mut rows = Vec::with_capacity(codes.len());
    for c in &codes {
        let name = match e.name(&c.place, &mut buf) {
            Ok(n) => n.map(str::to_owned),
            Err(teigeodata_engine::Error::MissingComponent { .. }) => None,
            Err(x) => return Err(x.into()),
        };
        rows.push(serde_json::json!({
            "scheme": c.scheme(),
            "code": c.code(),
            "kind": code_kind(c.kind),
            "valid_from": c.valid_from,
            "valid_to": c.valid_to,
            "replaced": c.is_replaced(),
            "id": c.place.id.to_string(),
            "name": name,
            "admin_level": c.place.admin_level,
        }));
    }
    if json {
        writeln!(out, "{}", serde_json::Value::Array(rows))?;
    } else {
        for (c, r) in codes.iter().zip(&rows) {
            writeln!(
                out,
                "{} {}\t{}\t{}\t{}{}",
                c.scheme(),
                c.code(),
                code_kind(c.kind),
                c.place.id,
                r["name"].as_str().unwrap_or("-"),
                if c.is_replaced() { "\t(replaced)" } else { "" }
            )?;
        }
    }
    if codes.is_empty() {
        return Err(Fail::Refused("no postal code".into()));
    }
    Ok(())
}
