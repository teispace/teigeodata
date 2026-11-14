//! The query parameters of the HTTP routes, parsed into the asks of [`crate::query`]: no runtime,
//! so the fuzz harness ([`crate::fuzz`]) runs the very code the server does.

use std::cell::RefCell;

use teigeodata_engine::{Engine, PostalCode, Scratch};

use crate::output::{self, Item, Mode};
use crate::query::{self, Ask, Code, Limits, PostalAsk, QueryError};
#[cfg(feature = "http")]
use crate::seams::Operation;

/// The routes under `/v1/`, in the order the fuzz harness picks them.
#[cfg(feature = "fuzz")]
pub(crate) const ROUTES: [&str; 13] = [
    "get",
    "lookup",
    "reverse",
    "near",
    "search",
    "admin",
    "tz",
    "elevation",
    "postal",
    "none",
    "nominatim/search",
    "nominatim/reverse",
    "nominatim/lookup",
];

/// A request's query parameters, decoded once.
pub(crate) struct Params(Vec<(String, String)>);

impl Params {
    pub(crate) fn parse(raw: Option<&str>) -> Self {
        Self(
            form_urlencoded::parse(raw.unwrap_or("").as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        )
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn text(&self, name: &str) -> Result<String, QueryError> {
        self.get(name)
            .map(str::to_owned)
            .ok_or_else(|| QueryError::new(Code::InvalidArgument, format!("`{name}` is missing")))
    }

    pub(crate) fn parsed<T: core::str::FromStr>(
        &self,
        name: &str,
    ) -> Result<Option<T>, QueryError> {
        self.get(name)
            .map(|v| {
                v.parse().map_err(|_| {
                    QueryError::new(
                        Code::InvalidArgument,
                        format!("`{name}` is not valid: {v:?}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn need<T: core::str::FromStr>(&self, name: &str) -> Result<T, QueryError> {
        self.parsed(name)?
            .ok_or_else(|| QueryError::new(Code::InvalidArgument, format!("`{name}` is missing")))
    }

    pub(crate) fn flag(&self, name: &str) -> Result<bool, QueryError> {
        match self.get(name) {
            None | Some("0" | "false") => Ok(false),
            Some("" | "1" | "true") => Ok(true),
            Some(v) => Err(QueryError::new(
                Code::InvalidArgument,
                format!("`{name}` is not a flag: {v:?}"),
            )),
        }
    }

    pub(crate) fn limit(&self) -> Result<usize, QueryError> {
        Ok(self.parsed("limit")?.unwrap_or(Limits::DEFAULT_LIMIT))
    }
}

/// What a `/v1/` route asks.
pub(crate) enum Asked {
    Places(Ask),
    Elevation(f64, f64),
    Postal(PostalAsk),
}

impl Asked {
    #[cfg(feature = "http")]
    pub(crate) const fn operation(&self) -> Operation {
        match self {
            Self::Places(a) => Operation::of(a),
            Self::Elevation(..) => Operation::Elevation,
            Self::Postal(_) => Operation::Postal,
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The ask of route `/v1/<route>` with parameters `p`.
pub(crate) fn asked(route: &str, p: &Params) -> Result<Asked, QueryError> {
    let ask = match route {
        "get" => Ask::Get {
            id: p.text("id")?,
            names: p.flag("names")?,
        },
        "lookup" => Ask::Lookup {
            ids: p
                .text("ids")?
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        },
        "reverse" => Ask::Reverse {
            lat: p.need("lat")?,
            lon: p.need("lon")?,
            limit: p.limit()?,
            in_country: p.flag("in_country")?,
        },
        "near" => Ask::Near {
            id: p.text("id")?,
            limit: p.limit()?,
            radius_m: p.parsed("radius_m")?,
        },
        "search" => Ask::Search {
            query: p.text("q")?,
            limit: p.limit()?,
            focus: match p.get("focus") {
                None => None,
                Some(f) => {
                    let bad = || {
                        QueryError::new(
                            Code::InvalidArgument,
                            format!("`focus` is not lat,lon: {f:?}"),
                        )
                    };
                    let (lat, lon) = f.split_once(',').ok_or_else(bad)?;
                    let scale_km: f64 = p.parsed("scale_km")?.unwrap_or(50.0);
                    Some((
                        lat.trim().parse().map_err(|_| bad())?,
                        lon.trim().parse().map_err(|_| bad())?,
                        scale_km * 1_000.0,
                    ))
                }
            },
            strict: p.flag("strict")?,
        },
        "admin" => Ask::Admin {
            lat: p.need("lat")?,
            lon: p.need("lon")?,
        },
        "tz" => {
            let t = p.parsed("t")?.unwrap_or_else(now);
            match p.get("id") {
                Some(id) => Ask::TzOf {
                    id: id.to_owned(),
                    t,
                },
                None => Ask::TzAt {
                    lat: p.need("lat")?,
                    lon: p.need("lon")?,
                    t,
                },
            }
        }
        "elevation" => return Ok(Asked::Elevation(p.need("lat")?, p.need("lon")?)),
        "postal" => {
            return Ok(Asked::Postal(match p.get("code") {
                Some(code) => PostalAsk::Code {
                    code: code.to_owned(),
                    scheme: p.get("scheme").map(str::to_owned),
                    prefix: p.flag("prefix")?,
                    limit: p.limit()?,
                },
                None => PostalAsk::At {
                    lat: p.need("lat")?,
                    lon: p.need("lon")?,
                },
            }));
        }
        _ => return Err(QueryError::new(Code::NotFound, "no such route")),
    };
    Ok(Asked::Places(ask))
}

thread_local! {
    /// Each worker's query memory, reused across requests (no allocation per query but the
    /// body).
    pub(crate) static WORK: RefCell<(Scratch, Vec<Item>, Vec<PostalCode>)> =
        RefCell::new((Scratch::default(), Vec::new(), Vec::new()));
}

/// Ends a native body: `"attribution": …}`.
fn close(out: &mut Vec<u8>, attribution: Option<&str>) {
    while out.last() == Some(&b'\n') {
        out.pop();
    }
    out.extend_from_slice(b",\"attribution\":");
    // A string or null cannot fail to serialize into a Vec.
    let _ = serde_json::to_writer(&mut *out, &attribution);
    out.extend_from_slice(b"}\n");
}

/// Runs `asked` on `e` and writes its body: the work of one request.
pub(crate) fn answer(
    e: &Engine,
    asked: &Asked,
    geojson: bool,
    limits: &Limits,
    out: &mut Vec<u8>,
) -> Result<usize, QueryError> {
    WORK.with_borrow_mut(|(s, items, codes)| {
        let fail = |x: output::OutputError| match x {
            output::OutputError::Engine(e) => QueryError::from(e),
            output::OutputError::Io(_) => QueryError::new(Code::Internal, "the answer failed"),
        };
        match asked {
            Asked::Places(ask) => {
                query::answer(e, ask, limits, s, items)?;
                if geojson {
                    output::write(e, items, Mode::Geojson, out).map_err(fail)?;
                } else {
                    out.extend_from_slice(b"{\"results\":");
                    output::write(e, items, Mode::Json, out).map_err(fail)?;
                    close(out, output::attribution_of(e, items).as_deref());
                }
                Ok(items.len())
            }
            Asked::Elevation(lat, lon) => {
                let h = query::elevation(e, *lat, *lon, s)?;
                out.extend_from_slice(b"{\"result\":");
                output::elevation(*lat, *lon, h.as_ref(), true, out).map_err(fail)?;
                close(
                    out,
                    output::attribution_layers(e, h.iter().map(|h| h.layer)).as_deref(),
                );
                Ok(usize::from(h.is_some()))
            }
            Asked::Postal(ask) => {
                query::postal(e, ask, limits, s, codes)?;
                out.extend_from_slice(b"{\"results\":");
                output::postal(e, codes, true, out).map_err(fail)?;
                close(
                    out,
                    output::attribution_layers(e, codes.iter().map(|c| c.place.layer)).as_deref(),
                );
                Ok(codes.len())
            }
        }
    })
}
