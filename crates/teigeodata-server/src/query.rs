//! The query model (phase 6.1): what a client may ask, the bounds every server applies, the
//! stable error codes, and the one place each query runs, for `tgd` and both servers alike.
//!
//! A query is checked against its [`Limits`] before the engine sees it, so its cost is bounded
//! by its inputs: the engine's calls are synchronous and short (D-309 runs them inline), and a
//! server's request timeout bounds the rest. `tgd` asks with [`Limits::NONE`]; a server with
//! [`Limits::SERVER`] or its own.

use core::fmt;

use teigeodata_core::{E6, E7, Grid, PlaceId, Point};
use teigeodata_engine::{
    CodeKind, Engine, Error, Focus, Height, Outline, Place, PostalCode, Scratch, SearchOptions,
};

use crate::output::{self, Item};

/// Why a query was not answered: a stable [`Code`] a client can act on, and a message.
#[derive(Debug)]
pub struct QueryError {
    /// What kind of failure, stable across releases.
    pub code: Code,
    /// What went wrong, for a person; never a file path or other server detail.
    pub message: String,
    /// The engine's error behind a [`Code::Internal`] or [`Code::MissingComponent`], for the
    /// server's own log and for `tgd`, which prints it whole.
    pub cause: Option<Error>,
}

/// The stable error codes of every server (and their HTTP status). New codes may be added;
/// a code's name and meaning never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Code {
    /// An argument does not parse or has no meaning (a bad ID, a latitude past 90°).
    InvalidArgument,
    /// An argument is past a bound of [`Limits`] (too long a query, too large a `limit`).
    OutOfBounds,
    /// The place asked for is not in the packs.
    NotFound,
    /// The packs served have no component that answers this (D-133).
    MissingComponent,
    /// The caller must say who they are (a hosted seam, ADR-0025).
    Unauthenticated,
    /// The caller may not ask this (a hosted seam).
    Forbidden,
    /// The caller asked too much, too fast (a hosted seam).
    RateLimited,
    /// The server cannot answer now; asking again later may work.
    Unavailable,
    /// The server failed: a pack could not be read. Never a client's fault.
    Internal,
}

impl Code {
    /// Its name on the wire: `snake_case`, stable.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid_argument",
            Self::OutOfBounds => "out_of_bounds",
            Self::NotFound => "not_found",
            Self::MissingComponent => "missing_component",
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal",
        }
    }

    /// Its HTTP status (RFC 9110).
    #[must_use]
    pub const fn http_status(self) -> u16 {
        match self {
            Self::InvalidArgument | Self::OutOfBounds => 400,
            Self::Unauthenticated => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::RateLimited => 429,
            Self::Internal => 500,
            // The server lacks what the request needs: not the client's fault, not transient.
            Self::MissingComponent => 501,
            Self::Unavailable => 503,
        }
    }
}

impl QueryError {
    /// An error of `code` with `message` and no engine cause.
    #[must_use]
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            cause: None,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|e| e as _)
    }
}

impl From<Error> for QueryError {
    /// A missing component names the component, never the layer that lacks it (the server's
    /// own detail); any other engine error is [`Code::Internal`] with a generic message.
    fn from(e: Error) -> Self {
        let (code, message) = match &e {
            Error::MissingComponent { component, .. } => (
                Code::MissingComponent,
                format!("the `{}` component is not installed", component.name()),
            ),
            _ => (Code::Internal, "a pack could not be read".to_owned()),
        };
        Self {
            code,
            message,
            cause: Some(e),
        }
    }
}

/// The bounds a query must keep. Each is a number with its gate (`tests/query.rs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// The longest text argument in bytes: a search query, an ID, a postal code.
    pub text_bytes: usize,
    /// The most results one query may ask for (`limit`, `k`), and the most IDs one lookup may
    /// name.
    pub limit: usize,
    /// The widest radius in metres.
    pub radius_m: f64,
    /// The most vertices one outline may answer: a finer one is answered at a coarser level of
    /// detail, and one past it at the coarsest is refused.
    pub vertices: usize,
}

impl Limits {
    /// A server's defaults: 256 bytes of text, 50 results, 1,000 km (servers note §7; Nominatim
    /// caps `limit` at 40 and its own query length similarly).
    pub const SERVER: Self = Self {
        text_bytes: 256,
        limit: 50,
        radius_m: 1_000_000.0,
        vertices: 20_000,
    };

    /// No bound: `tgd`, whose user owns the machine it runs on.
    pub const NONE: Self = Self {
        text_bytes: usize::MAX,
        limit: usize::MAX,
        radius_m: f64::INFINITY,
        vertices: usize::MAX,
    };

    /// The results a query answers when it does not say.
    pub const DEFAULT_LIMIT: usize = 10;

    fn text(&self, what: &str, text: &str) -> Result<(), QueryError> {
        if text.len() > self.text_bytes {
            return Err(QueryError::new(
                Code::OutOfBounds,
                format!(
                    "{what} is {} bytes, at most {} are allowed",
                    text.len(),
                    self.text_bytes
                ),
            ));
        }
        Ok(())
    }

    fn count(&self, n: usize) -> Result<(), QueryError> {
        if n > self.limit {
            return Err(QueryError::new(
                Code::OutOfBounds,
                format!("limit {n} is past the most allowed, {}", self.limit),
            ));
        }
        Ok(())
    }

    fn radius(&self, m: f64) -> Result<(), QueryError> {
        if !(m > 0.0 && m <= self.radius_m) {
            return Err(QueryError::new(
                Code::OutOfBounds,
                format!("radius {m} m is outside (0, {}] m", self.radius_m),
            ));
        }
        Ok(())
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::SERVER
    }
}

/// A point from degrees on grid `G`.
///
/// # Errors
/// [`Code::InvalidArgument`] for a coordinate that is not finite or not on the globe.
pub fn point<G: Grid>(lat: f64, lon: f64) -> Result<Point<G>, QueryError> {
    Point::from_degrees(lat, lon)
        .map_err(|e| QueryError::new(Code::InvalidArgument, format!("{lat},{lon}: {e}")))
}

/// A place ID from its text form.
///
/// # Errors
/// [`Code::InvalidArgument`] for text that is not an ID.
pub fn id(text: &str) -> Result<PlaceId, QueryError> {
    text.parse()
        .map_err(|e| QueryError::new(Code::InvalidArgument, format!("{text}: {e}")))
}

/// What a client asks about places. Each answers a list of [`Item`]s, printed by
/// [`output::write`].
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// One place by ID; with `names`, every name it has.
    Get {
        /// The ID's text form.
        id: String,
        /// List every name with its language and origin (D-227).
        names: bool,
    },
    /// Places by ID, in the order asked; IDs not in the packs are left out (Nominatim's
    /// `/lookup`).
    Lookup {
        /// The IDs' text forms.
        ids: Vec<String>,
    },
    /// The places nearest a point, each with its distance and bearing and whether it holds the
    /// point.
    Reverse {
        /// Latitude, degrees.
        lat: f64,
        /// Longitude, degrees.
        lon: f64,
        /// How many.
        limit: usize,
        /// Only places in the country holding the point (TG-19).
        in_country: bool,
    },
    /// The places nearest a place, or within a radius of it.
    Near {
        /// The place's ID.
        id: String,
        /// How many.
        limit: usize,
        /// Within this many metres, if given.
        radius_m: Option<f64>,
    },
    /// Places by name, best first.
    Search {
        /// The text.
        query: String,
        /// How many.
        limit: usize,
        /// Rank nearer places higher: latitude, longitude, scale in metres.
        focus: Option<(f64, f64, f64)>,
        /// Strict order: every word in order.
        strict: bool,
    },
    /// A place's ancestors, its parent first, then the parent's, to the root (D-143).
    Hierarchy {
        /// The place's ID.
        id: String,
    },
    /// The places whose parent is a place, in the packs' order; the first `limit` of them.
    Children {
        /// The place's ID.
        id: String,
        /// How many.
        limit: usize,
    },
    /// The admin units holding a point, country first.
    Admin {
        /// Latitude, degrees.
        lat: f64,
        /// Longitude, degrees.
        lon: f64,
    },
    /// The timezone area at a point, with its UTC offset at `t` when the rules are installed.
    TzAt {
        /// Latitude, degrees.
        lat: f64,
        /// Longitude, degrees.
        lon: f64,
        /// Unix seconds.
        t: i64,
    },
    /// A place's timezone area, with its UTC offset at `t`.
    TzOf {
        /// The place's ID.
        id: String,
        /// Unix seconds.
        t: i64,
    },
}

impl Ask {
    /// Checks the arguments against `limits` without running anything.
    ///
    /// # Errors
    /// [`Code::OutOfBounds`] for an argument past a bound.
    pub fn check(&self, limits: &Limits) -> Result<(), QueryError> {
        match self {
            Self::Get { id, .. } | Self::TzOf { id, .. } | Self::Hierarchy { id } => {
                limits.text("the ID", id)
            }
            Self::Children { id, limit } => {
                limits.text("the ID", id)?;
                limits.count(*limit)
            }
            Self::Lookup { ids } => {
                limits.count(ids.len())?;
                ids.iter().try_for_each(|i| limits.text("an ID", i))
            }
            Self::Reverse { limit, .. } => limits.count(*limit),
            Self::Near {
                id,
                limit,
                radius_m,
            } => {
                limits.text("the ID", id)?;
                limits.count(*limit)?;
                radius_m.map_or(Ok(()), |m| limits.radius(m))
            }
            Self::Search {
                query,
                limit,
                focus,
                ..
            } => {
                limits.text("the query", query)?;
                limits.count(*limit)?;
                focus.map_or(Ok(()), |(_, _, scale)| {
                    if scale.is_finite() && scale > 0.0 {
                        Ok(())
                    } else {
                        Err(QueryError::new(
                            Code::InvalidArgument,
                            format!("the focus scale must be positive: {scale} m"),
                        ))
                    }
                })
            }
            Self::Admin { .. } | Self::TzAt { .. } => Ok(()),
        }
    }
}

/// The place `text` names, or [`Code::NotFound`].
fn place(e: &Engine, text: &str) -> Result<Place, QueryError> {
    e.get(id(text)?)?
        .ok_or_else(|| QueryError::new(Code::NotFound, format!("{text}: not found")))
}

/// Answers `ask` within `limits` into `out` (cleared first).
///
/// # Errors
/// The [`Ask::check`] errors, an argument that does not parse, a place asked by ID that is not
/// there, or the engine's.
pub fn answer(
    e: &Engine,
    ask: &Ask,
    limits: &Limits,
    s: &mut Scratch,
    out: &mut Vec<Item>,
) -> Result<(), QueryError> {
    ask.check(limits)?;
    out.clear();
    match ask {
        Ask::Get { id, names } => {
            let mut item = Item::from(place(e, id)?);
            item.all_names = *names;
            out.push(item);
        }
        Ask::Lookup { ids } => {
            for text in ids {
                if let Some(p) = e.get(id(text)?)? {
                    out.push(Item::from(p));
                }
            }
        }
        Ask::Reverse {
            lat,
            lon,
            limit,
            in_country,
        } => {
            let at = point::<E6>(*lat, *lon)?;
            let mut near = Vec::new();
            if *in_country {
                e.reverse_in_country(at, *limit, s, &mut near)?;
            } else {
                e.reverse(at, *limit, s, &mut near)?;
            }
            e.mark_inside(at, s, &mut near)?;
            out.extend(near.iter().map(|n| Item {
                meters: Some(n.meters),
                bearing: Some(n.bearing_deg),
                inside: n.inside,
                ..Item::from(n.place)
            }));
        }
        Ask::Near {
            id,
            limit,
            radius_m,
        } => near(e, &place(e, id)?, *limit, *radius_m, s, out)?,
        Ask::Search {
            query,
            limit,
            focus,
            strict,
        } => {
            let mut opts = SearchOptions::default().with_strict(*strict);
            if let Some((lat, lon, scale_m)) = *focus {
                let mut f = Focus::new(point(lat, lon)?);
                f.scale_m = scale_m;
                opts = opts.with_focus(f);
            }
            let mut hits = Vec::new();
            e.search_with(query, *limit, &opts, s, &mut hits)?;
            out.extend(hits.iter().map(|h| Item {
                score: Some(h.score),
                ..Item::from(h.place)
            }));
        }
        Ask::Hierarchy { id } => {
            let mut up = Vec::new();
            e.ancestors(&place(e, id)?, &mut up)?;
            out.extend(up.into_iter().map(Item::from));
        }
        Ask::Children { id, limit } => {
            let mut down = Vec::new();
            e.children(&place(e, id)?, &mut down)?;
            out.extend(down.into_iter().take(*limit).map(Item::from));
        }
        Ask::Admin { lat, lon } => {
            let mut units = Vec::new();
            e.admin_at(point::<E7>(*lat, *lon)?, s, &mut units)?;
            out.extend(units.into_iter().map(Item::from));
        }
        Ask::TzAt { lat, lon, t } => {
            let area = e.tz_at(point::<E7>(*lat, *lon)?)?;
            with_offset(e, area, *t, out)?;
        }
        Ask::TzOf { id, t } => {
            let area = e.tz_of(&place(e, id)?)?;
            with_offset(e, area, *t, out)?;
        }
    }
    Ok(())
}

/// The places nearest `p`, or within `radius_m` of it, itself left out.
fn near(
    e: &Engine,
    p: &Place,
    limit: usize,
    radius_m: Option<f64>,
    s: &mut Scratch,
    out: &mut Vec<Item>,
) -> Result<(), QueryError> {
    let mut near = Vec::new();
    match radius_m {
        Some(m) => {
            // The place itself is at 0 m: ask for one more and drop it.
            e.within_radius(p.point, m, limit.saturating_add(1), s, &mut near)?;
            near.retain(|n| n.place.id != p.id);
            near.truncate(limit);
        }
        None => e.nearby(p, limit, s, &mut near)?,
    }
    out.extend(near.iter().map(|n| Item {
        meters: Some(n.meters),
        bearing: Some(n.bearing_deg),
        ..Item::from(n.place)
    }));
    Ok(())
}

/// A timezone area with the UTC offset in force at `t` when its rules are installed (TG-03).
fn with_offset(
    e: &Engine,
    area: Option<Place>,
    t: i64,
    out: &mut Vec<Item>,
) -> Result<(), QueryError> {
    if let Some(a) = area {
        let mut item = Item::from(a);
        if let Some(zone) = e.zone_name(&a)?
            && let Some(o) = output::installed(e.tz_offset_at(zone, t))?
        {
            item.offset = Some(o);
        }
        out.push(item);
    }
    Ok(())
}

/// The terrain height at a point from the packs' DEM (D-192), or `None` where there is none.
///
/// # Errors
/// A coordinate off the globe, or the engine's.
pub fn elevation(
    e: &Engine,
    lat: f64,
    lon: f64,
    s: &mut Scratch,
) -> Result<Option<Height>, QueryError> {
    Ok(e.elevation_at(point::<E7>(lat, lon)?, s)?)
}

/// One end of a distance: a place by ID, or a point.
#[derive(Clone, Debug, PartialEq)]
pub enum At {
    /// A place's ID.
    Place(String),
    /// A point, degrees.
    Point {
        /// Latitude.
        lat: f64,
        /// Longitude.
        lon: f64,
    },
}

/// One end of a measured distance: the point, and the place when it was one.
#[derive(Clone, Copy, Debug)]
pub struct End {
    /// Latitude, degrees.
    pub lat: f64,
    /// Longitude, degrees.
    pub lon: f64,
    /// The place, when the end was asked by ID.
    pub place: Option<Place>,
}

/// A great-circle distance between two ends.
#[derive(Clone, Copy, Debug)]
pub struct Measured {
    /// Where it starts.
    pub from: End,
    /// Where it ends.
    pub to: End,
    /// Metres on the mean Earth sphere (within about 0.5% of the ellipsoid).
    pub meters: f64,
    /// The initial bearing from `from` to `to`, degrees clockwise from north.
    pub bearing_deg: f64,
}

impl At {
    /// An end from its text: `lat,lon` for a point, anything else a place's ID.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let point = text.split_once(',').and_then(|(lat, lon)| {
            Some(Self::Point {
                lat: lat.trim().parse().ok()?,
                lon: lon.trim().parse().ok()?,
            })
        });
        point.unwrap_or_else(|| Self::Place(text.to_owned()))
    }

    fn end(&self, e: &Engine, limits: &Limits) -> Result<End, QueryError> {
        match self {
            Self::Place(id) => {
                limits.text("the ID", id)?;
                let p = place(e, id)?;
                Ok(End {
                    lat: p.point.lat_degrees(),
                    lon: p.point.lon_degrees(),
                    place: Some(p),
                })
            }
            Self::Point { lat, lon } => {
                point::<E7>(*lat, *lon)?;
                Ok(End {
                    lat: *lat,
                    lon: *lon,
                    place: None,
                })
            }
        }
    }
}

/// The great-circle distance and initial bearing from `from` to `to`.
///
/// # Errors
/// A place not found, an ID past the bound, a point off the globe.
pub fn distance(e: &Engine, from: &At, to: &At, limits: &Limits) -> Result<Measured, QueryError> {
    let (a, b) = (from.end(e, limits)?, to.end(e, limits)?);
    Ok(Measured {
        from: a,
        to: b,
        meters: teigeodata_engine::meters_between(a.lat, a.lon, b.lat, b.lon),
        bearing_deg: teigeodata_engine::bearing_deg(a.lat, a.lon, b.lat, b.lon),
    })
}

/// The coarsest level of detail [`boundary`] reads; 0 is full resolution.
pub const COARSEST_LOD: u8 = 6;

/// A place's outline as answered: the level of detail read, into the caller's [`Outline`].
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// The place.
    pub place: Place,
    /// The level of detail answered: the one asked, or coarser to keep within
    /// [`Limits::vertices`]; 0 from a pack stored at full resolution only.
    pub lod: u8,
    /// Whether the place has an outline; a point place has none (its point is its extent).
    pub outlined: bool,
}

/// The outline of the place `id` at level of detail `lod` (0, full resolution, when `None`)
/// into `out`, or at the first coarser level within [`Limits::vertices`].
///
/// # Errors
/// A `lod` past [`COARSEST_LOD`], a place not found, an ID past the bound, an outline past
/// the bound at the coarsest level, or the engine's (`boundaries` not installed).
pub fn boundary(
    e: &Engine,
    id: &str,
    lod: Option<u8>,
    limits: &Limits,
    out: &mut Outline,
) -> Result<Shape, QueryError> {
    limits.text("the ID", id)?;
    let first = lod.unwrap_or(0);
    if first > COARSEST_LOD {
        return Err(QueryError::new(
            Code::InvalidArgument,
            format!("`lod` must be 0 to {COARSEST_LOD}"),
        ));
    }
    let p = place(e, id)?;
    // The level is chosen from the arcs' levels alone; only the one chosen is decoded.
    let mut last = None;
    let mut chosen = None;
    for l in first..=COARSEST_LOD {
        let Some(n) = e.boundary_vertices(&p, l)? else {
            e.boundary(&p, l, out)?;
            return Ok(Shape {
                place: p,
                lod: l,
                outlined: false,
            });
        };
        if n <= limits.vertices {
            chosen = Some(l);
            break;
        }
        // A pack stored at full resolution only has no coarser level: stop at once.
        if last == Some(n) {
            break;
        }
        last = Some(n);
    }
    let Some(l) = chosen else {
        return Err(QueryError::new(
            Code::OutOfBounds,
            format!(
                "{id}: the outline has {} vertices at its coarsest, past the most allowed, {}",
                last.unwrap_or_default(),
                limits.vertices
            ),
        ));
    };
    e.boundary(&p, l, out)?;
    Ok(Shape {
        place: p,
        // Only full resolution has no tolerance: a pack without levels reads it at any.
        lod: if out.tolerance_cm == 0 { 0 } else { l },
        outlined: true,
    })
}

/// What a client asks about postal codes (D-200).
#[derive(Clone, Debug, PartialEq)]
pub enum PostalAsk {
    /// A code to its places; with `prefix`, up to `limit` codes starting with it.
    Code {
        /// The code, or its start.
        code: String,
        /// Only this scheme (`NP`, `IN-PIN`), if given.
        scheme: Option<String>,
        /// Codes starting with `code`.
        prefix: bool,
        /// How many codes, for a prefix.
        limit: usize,
    },
    /// The codes at a point.
    At {
        /// Latitude, degrees.
        lat: f64,
        /// Longitude, degrees.
        lon: f64,
    },
}

/// Answers a postal `ask` within `limits` into `out` (cleared first).
///
/// # Errors
/// An argument past a bound, a coordinate off the globe, or the engine's.
pub fn postal(
    e: &Engine,
    ask: &PostalAsk,
    limits: &Limits,
    s: &mut Scratch,
    out: &mut Vec<PostalCode>,
) -> Result<(), QueryError> {
    out.clear();
    match ask {
        PostalAsk::Code {
            code,
            scheme,
            prefix,
            limit,
        } => {
            limits.text("the code", code)?;
            if let Some(sc) = scheme {
                limits.text("the scheme", sc)?;
            }
            if *prefix {
                limits.count(*limit)?;
                e.postal_prefix(code, scheme.as_deref(), *limit, out)?;
            } else {
                e.postal_lookup(code, scheme.as_deref(), out)?;
            }
        }
        PostalAsk::At { lat, lon } => {
            e.postal_at(point::<E7>(*lat, *lon)?, s, out)?;
        }
    }
    Ok(())
}

/// A postal code kind's name on the wire.
#[must_use]
pub const fn code_kind(kind: CodeKind) -> &'static str {
    match kind {
        CodeKind::Area => "area",
        CodeKind::Part => "part",
        CodeKind::PostOffice => "post_office",
        CodeKind::Point => "point",
    }
}
