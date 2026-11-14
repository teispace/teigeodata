//! The query parameters of the HTTP routes, parsed into the asks of [`crate::query`]: no runtime,
//! so the fuzz harness ([`crate::fuzz`]) runs the very code the server does.

use crate::body::Asked;
use crate::query::{Ask, At, Code, Limits, PostalAsk, QueryError};

/// The routes under `/v1/`, in the order the fuzz harness picks them.
#[cfg(feature = "fuzz")]
pub(crate) const ROUTES: [&str; 18] = [
    "get",
    "lookup",
    "reverse",
    "near",
    "search",
    "hierarchy",
    "children",
    "admin",
    "tz",
    "elevation",
    "postal",
    "boundary",
    "distance",
    "coverage",
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

/// The ask of route `/v1/<route>` with parameters `p`.
pub(crate) fn asked(route: &str, p: &Params) -> Result<Asked, QueryError> {
    if let Some(other) = not_places(route, p)? {
        return Ok(other);
    }
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
        "hierarchy" => Ask::Hierarchy { id: p.text("id")? },
        "children" => Ask::Children {
            id: p.text("id")?,
            limit: p.limit()?,
        },
        "admin" => Ask::Admin {
            lat: p.need("lat")?,
            lon: p.need("lon")?,
        },
        "tz" => {
            let t = p.parsed("t")?.unwrap_or_else(crate::body::now);
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
        _ => return Err(QueryError::new(Code::NotFound, "no such route")),
    };
    Ok(Asked::Places(ask))
}

/// The asks of the routes that answer no list of places, or `None` for a place route.
fn not_places(route: &str, p: &Params) -> Result<Option<Asked>, QueryError> {
    Ok(Some(match route {
        "elevation" => Asked::Elevation(p.need("lat")?, p.need("lon")?),
        "boundary" => Asked::Boundary(p.text("id")?, p.parsed("lod")?),
        "coverage" => Asked::Coverage(p.need("lat")?, p.need("lon")?, p.parsed("radius_m")?),
        "distance" => Asked::Distance(At::parse(&p.text("from")?), At::parse(&p.text("to")?)),
        "postal" => Asked::Postal(match p.get("code") {
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
        }),
        _ => return Ok(None),
    }))
}
