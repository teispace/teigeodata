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

/// A route of the HTTP server as its reference documents it (phase 7.3): the table the
/// reference page is generated from. A test holds it to the parser: every parameter a route
/// reads is listed, every one listed is read, each example parses, and a required one left out
/// fails.
#[cfg(feature = "http")]
#[derive(Clone, Copy, Debug)]
pub struct Endpoint {
    /// The path: `/v1/search`, `/search`.
    pub path: &'static str,
    /// What it answers, for a person.
    pub doc: &'static str,
    /// Its query parameters.
    pub params: &'static [Param],
    /// Query strings it answers, one per form of the route.
    pub examples: &'static [&'static str],
}

/// A query parameter of an [`Endpoint`].
#[cfg(feature = "http")]
#[derive(Clone, Copy, Debug)]
pub struct Param {
    /// Its name.
    pub name: &'static str,
    /// Whether the route fails without it (an alternative of a route with two forms is not).
    pub required: bool,
    /// Its values and default, for a person.
    pub doc: &'static str,
}

#[cfg(feature = "http")]
const fn req(name: &'static str, doc: &'static str) -> Param {
    Param {
        name,
        required: true,
        doc,
    }
}

#[cfg(feature = "http")]
const fn opt(name: &'static str, doc: &'static str) -> Param {
    Param {
        name,
        required: false,
        doc,
    }
}

#[cfg(feature = "http")]
const LAT: Param = req("lat", "Latitude, WGS84 degrees, -90 to 90.");
#[cfg(feature = "http")]
const LON: Param = req("lon", "Longitude, WGS84 degrees, -180 to 180.");
#[cfg(feature = "http")]
const ID: Param = req("id", "A place ID, such as `tgd:np/1xhb`.");
#[cfg(feature = "http")]
const LIMIT: Param = opt(
    "limit",
    "How many results: 10 by default, at most the server's bound (50).",
);

/// The query parameter every native place route takes besides its own: `format=json` (the
/// default) or `format=geojson` for a GeoJSON FeatureCollection.
#[cfg(feature = "http")]
pub const FORMAT: Param = opt(
    "format",
    "`json` (the default) or `geojson`: a FeatureCollection (RFC 7946) of the places.",
);

/// Every route that takes query parameters, native (`/v1/…`) first, then the Nominatim ones.
#[cfg(feature = "http")]
pub const ENDPOINTS: &[Endpoint] = &[
    Endpoint {
        path: "/v1/search",
        doc: "Places matching a name in any script or spelling, best first, each with its `score`. \"Name, Parent\" ranks places under a parent of that name first.",
        params: &[
            req(
                "q",
                "The name to find, such as `Pokhara`, `पोखरा` or `Pokhara, Kaski`.",
            ),
            LIMIT,
            opt(
                "focus",
                "`lat,lon`: rank places near this point higher (30 points at the point, halving every `scale_km`).",
            ),
            opt(
                "scale_km",
                "How far counts as near the focus, in km: 50 by default. Read only with `focus`.",
            ),
            opt(
                "strict",
                "`1`: rank by how well the name matched before anything else.",
            ),
        ],
        examples: &[
            "q=Pokhara&limit=3&strict=1",
            "q=Pokhara&focus=28.2,83.9&scale_km=20",
        ],
    },
    Endpoint {
        path: "/v1/get",
        doc: "One place by its ID; a retired ID answers its survivor.",
        params: &[
            ID,
            opt(
                "names",
                "`1`: add every name of the place, with its language and origin.",
            ),
        ],
        examples: &["id=tgd:np/1xhb&names=1"],
    },
    Endpoint {
        path: "/v1/lookup",
        doc: "Places by ID, in the order asked; IDs not found are left out.",
        params: &[req(
            "ids",
            "Comma-separated place IDs, at most the server's `limit` bound.",
        )],
        examples: &["ids=tgd:np/1xhb,tgd:np/1wmc"],
    },
    Endpoint {
        path: "/v1/reverse",
        doc: "The places nearest a point, nearest first, each with `meters`, `bearing` (degrees clockwise from north) and `inside` (its outline holds the point).",
        params: &[
            LAT,
            LON,
            LIMIT,
            opt(
                "in_country",
                "`1`: only places in the country that holds the point.",
            ),
        ],
        examples: &["lat=28.21&lon=83.99&limit=3&in_country=1"],
    },
    Endpoint {
        path: "/v1/near",
        doc: "The places nearest a place, nearest first, the place itself left out.",
        params: &[
            ID,
            LIMIT,
            opt(
                "radius_m",
                "Only places within this many metres (at most 1,000,000).",
            ),
        ],
        examples: &["id=tgd:np/1xhb&limit=5&radius_m=5000"],
    },
    Endpoint {
        path: "/v1/admin",
        doc: "The admin units holding a point: the country first, then each finer level.",
        params: &[LAT, LON],
        examples: &["lat=27.7172&lon=85.324"],
    },
    Endpoint {
        path: "/v1/hierarchy",
        doc: "A place's ancestors: its parent first, up to the country.",
        params: &[ID],
        examples: &["id=tgd:np/1xhb"],
    },
    Endpoint {
        path: "/v1/children",
        doc: "A place's children, by ID.",
        params: &[ID, LIMIT],
        examples: &["id=tgd:np/1wmc&limit=20"],
    },
    Endpoint {
        path: "/v1/tz",
        doc: "The timezone of a place or a point, with its UTC offset, abbreviation and daylight-saving flag at an instant.",
        params: &[
            opt("id", "A place ID; or give `lat` and `lon`."),
            opt("lat", "Latitude, with `lon`, when no `id` is given."),
            opt("lon", "Longitude, with `lat`."),
            opt("t", "The instant, in Unix seconds: now by default."),
        ],
        examples: &["id=tgd:np/1xhb&t=1767225600", "lat=27.7172&lon=85.324"],
    },
    Endpoint {
        path: "/v1/elevation",
        doc: "The terrain height at a point in metres above EGM2008, with the grid's source and spacing. One value, under `result`.",
        params: &[LAT, LON],
        examples: &["lat=27.9881&lon=86.925"],
    },
    Endpoint {
        path: "/v1/distance",
        doc: "The great-circle distance in metres and the initial bearing between two points or places. One value, under `result`.",
        params: &[
            req("from", "`lat,lon` or a place ID."),
            req("to", "`lat,lon` or a place ID."),
        ],
        examples: &["from=27.7172,85.324&to=tgd:np/1xhb"],
    },
    Endpoint {
        path: "/v1/postal",
        doc: "The places a postal code names, codes starting with a prefix, or the codes at a point.",
        params: &[
            opt(
                "code",
                "A postal code, with or without separators, in any case or script's digits; or give `lat` and `lon`.",
            ),
            opt("scheme", "The scheme, when a country has more than one."),
            opt("prefix", "`1`: complete `code` as a prefix."),
            opt("limit", "How many results with `code`: 10 by default."),
            opt("lat", "Latitude, with `lon`, when no `code` is given."),
            opt("lon", "Longitude, with `lat`."),
        ],
        examples: &[
            "code=44600&prefix=1&limit=5&scheme=np",
            "lat=27.7172&lon=85.324",
        ],
    },
    Endpoint {
        path: "/v1/boundary",
        doc: "An admin unit's outline as a GeoJSON Feature, in any `format`.",
        params: &[
            ID,
            opt(
                "lod",
                "The level of detail, 0 (finest, the default) to 6 (coarsest).",
            ),
        ],
        examples: &["id=tgd:np/1wmc&lod=3"],
    },
    Endpoint {
        path: "/v1/coverage",
        doc: "The catalog's packs that cover a point, with their sizes: what to install for it. A server over a store only.",
        params: &[
            LAT,
            LON,
            opt("radius_m", "Also the packs within this many metres."),
        ],
        examples: &["lat=25.2048&lon=55.2708&radius_m=10000"],
    },
    Endpoint {
        path: "/search",
        doc: "Nominatim's search: a JSON array of Nominatim places.",
        params: &[
            opt("q", "Free-form text; or the structured fields below."),
            opt(
                "amenity",
                "Structured search: joined with the other fields, most specific first.",
            ),
            opt("street", "Structured search."),
            opt("city", "Structured search."),
            opt("county", "Structured search."),
            opt("state", "Structured search."),
            opt("country", "Structured search."),
            opt("postalcode", "Structured search."),
            opt("limit", "How many results: 10 by default, at most 40."),
            opt(
                "countrycodes",
                "Comma-separated ISO 3166-1 alpha-2 codes to keep.",
            ),
            opt(
                "viewbox",
                "`x1,y1,x2,y2` (longitude first): rank places in the box higher.",
            ),
            opt("bounded", "`1`: only places in `viewbox`."),
            opt("addressdetails", "`1`: add each place's `address`."),
            opt(
                "format",
                "`json` (the default), `jsonv2` (the same) or `geojson`.",
            ),
            opt(
                "json_callback",
                "Refused: JSONP is not supported; call with CORS.",
            ),
        ],
        examples: &[
            "q=Pokhara&limit=3&countrycodes=np&viewbox=83.8,28.1,84.1,28.3&bounded=1&addressdetails=1&format=jsonv2",
            "city=Pokhara&country=Nepal",
        ],
    },
    Endpoint {
        path: "/reverse",
        doc: "Nominatim's reverse: the place at a point at a zoom's depth, or `{\"error\": \"Unable to geocode\"}`.",
        params: &[
            LAT,
            LON,
            opt(
                "zoom",
                "How deep: 3 a country, 5 to 7 a first-level unit, 8 and 9 the next, 10 to 18 the nearest place within reach; 18 by default.",
            ),
            opt(
                "addressdetails",
                "`0` leaves out `address` (on by default).",
            ),
            opt(
                "format",
                "`json` (the default), `jsonv2` (the same) or `geojson`.",
            ),
            opt("json_callback", "Refused: JSONP is not supported."),
        ],
        examples: &["lat=28.21&lon=83.99&zoom=10&addressdetails=0&format=geojson"],
    },
    Endpoint {
        path: "/lookup",
        doc: "Nominatim's lookup: places by ID, as a JSON array.",
        params: &[
            req(
                "osm_ids",
                "Comma-separated teigeodata IDs, at most 50; an OSM reference such as `N123` is left out as not found.",
            ),
            opt("addressdetails", "`1`: add each place's `address`."),
            opt(
                "format",
                "`json` (the default), `jsonv2` (the same) or `geojson`.",
            ),
            opt("json_callback", "Refused: JSONP is not supported."),
        ],
        examples: &["osm_ids=tgd:np/1xhb,N123&addressdetails=1"],
    },
];

/// A request's query parameters, decoded once.
pub(crate) struct Params(
    Vec<(String, String)>,
    /// The names read, for the test that holds [`ENDPOINTS`] to the parser.
    #[cfg(test)]
    core::cell::RefCell<Vec<String>>,
);

impl Params {
    pub(crate) fn parse(raw: Option<&str>) -> Self {
        Self(
            form_urlencoded::parse(raw.unwrap_or("").as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            #[cfg(test)]
            core::cell::RefCell::default(),
        )
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        #[cfg(test)]
        self.1.borrow_mut().push(name.to_owned());
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

#[cfg(all(test, feature = "http"))]
mod tests {
    #![allow(clippy::panic, reason = "tests: failures panic with their message")]

    use std::collections::BTreeSet;

    use super::{ENDPOINTS, Endpoint, Params, asked};
    use crate::compat::{self, Route};
    use crate::query::Limits;

    /// Asks endpoint `e` with query `q` on an engine with no packs: whether the parameters
    /// parse, and the names read.
    fn run(e: &Endpoint, q: &str) -> (Result<(), String>, BTreeSet<String>) {
        let p = Params::parse(Some(q));
        let r = if let Some(route) = e.path.strip_prefix("/v1/") {
            asked(route, &p).map(|_| ())
        } else {
            let route = match e.path {
                "/search" => Route::Search,
                "/reverse" => Route::Reverse,
                "/lookup" => Route::Lookup,
                other => panic!("no such route: {other}"),
            };
            let engine = teigeodata_engine::Engine::new();
            compat::answer(&engine, route, &p, &Limits::SERVER, &mut Vec::new()).map(|_| ())
        };
        (r.map_err(|x| x.message), p.1.take().into_iter().collect())
    }

    /// `q` without parameter `name`.
    fn without(q: &str, name: &str) -> String {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_urlencoded::parse(q.as_bytes()).filter(|(k, _)| k != name))
            .finish()
    }

    #[test]
    fn the_reference_lists_what_each_route_reads() {
        let mut paths = BTreeSet::new();
        for e in ENDPOINTS {
            assert!(paths.insert(e.path), "{} is listed twice", e.path);
            let listed: BTreeSet<String> = e.params.iter().map(|p| p.name.to_owned()).collect();
            assert_eq!(
                listed.len(),
                e.params.len(),
                "{}: a parameter twice",
                e.path
            );
            let mut read = BTreeSet::new();
            for q in e.examples {
                let (r, seen) = run(e, q);
                assert!(r.is_ok(), "{} ?{q}: {r:?}", e.path);
                read.extend(seen);
                for p in e.params.iter().filter(|p| p.required) {
                    let (r, _) = run(e, &without(q, p.name));
                    assert!(r.is_err(), "{} answers without `{}`", e.path, p.name);
                }
            }
            assert_eq!(
                read, listed,
                "{}: the parameters it reads (left) are not those listed (right)",
                e.path
            );
        }
    }

    /// The fuzz harness picks from every documented route.
    #[cfg(feature = "fuzz")]
    #[test]
    fn the_fuzz_harness_reaches_every_route() {
        let fuzzed: BTreeSet<String> = super::ROUTES
            .iter()
            .filter(|r| **r != "none")
            .map(|r| match r.strip_prefix("nominatim") {
                Some(n) => n.to_owned(),
                None => format!("/v1/{r}"),
            })
            .collect();
        let listed: BTreeSet<String> = ENDPOINTS.iter().map(|e| e.path.to_owned()).collect();
        assert_eq!(fuzzed, listed);
    }
}
