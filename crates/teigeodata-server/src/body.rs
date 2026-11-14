//! The body of an answer, the same for every front end: the native envelope
//! `{"results": [...], "attribution": ...}` (D-032) or GeoJSON, written by the one code path
//! the HTTP routes, the MCP tools and the fuzz harness share. No runtime.

use std::cell::RefCell;
use std::path::Path;

use teigeodata_engine::{Engine, Outline, PostalCode, Scratch};

use crate::output::{self, Item, Mode};
use crate::query::{self, Ask, At, Code, Limits, PostalAsk, QueryError};
use crate::seams::Operation;

/// What a request asks: places, a height or postal codes.
pub(crate) enum Asked {
    Places(Ask),
    Elevation(f64, f64),
    Postal(PostalAsk),
    /// A place's outline, at a level of detail.
    Boundary(String, Option<u8>),
    /// A distance between two ends.
    Distance(At, At),
    /// The countries at or near a point and the catalog's packs for it: latitude, longitude,
    /// radius in metres.
    Coverage(f64, f64, Option<u32>),
}

impl Asked {
    #[cfg_attr(
        not(any(feature = "http", feature = "mcp")),
        allow(dead_code, reason = "the fuzz harness asks no seam")
    )]
    pub(crate) const fn operation(&self) -> Operation {
        match self {
            Self::Places(a) => Operation::of(a),
            Self::Elevation(..) => Operation::Elevation,
            Self::Postal(_) => Operation::Postal,
            Self::Boundary(..) => Operation::Boundary,
            Self::Distance(..) => Operation::Distance,
            Self::Coverage(..) => Operation::Coverage,
        }
    }
}

/// Now, in Unix seconds: the instant a timezone query asks about when it names none.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

thread_local! {
    /// Each worker's query memory, reused across requests (no allocation per query but the
    /// body).
    pub(crate) static WORK: RefCell<(Scratch, Vec<Item>, Vec<PostalCode>)> =
        RefCell::new((Scratch::default(), Vec::new(), Vec::new()));
    /// Each worker's outline memory: a country's outline is large, read once per request.
    pub(crate) static SHAPE: RefCell<Outline> = RefCell::new(Outline::default());
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

/// The part of a place answer to write: `skip` places, then at most `take` (MCP's pages; the
/// HTTP routes write everything).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Page {
    pub(crate) skip: usize,
    pub(crate) take: usize,
}

impl Page {
    pub(crate) const ALL: Self = Self {
        skip: 0,
        take: usize::MAX,
    };
}

/// What [`answer`] wrote: how many results, and whether places were left past the page.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Answered {
    #[cfg_attr(
        not(any(feature = "http", feature = "mcp")),
        allow(dead_code, reason = "the fuzz harness meters nothing")
    )]
    pub(crate) results: usize,
    #[cfg_attr(
        not(feature = "mcp"),
        allow(dead_code, reason = "only MCP's pages ask whether more are there")
    )]
    pub(crate) more: bool,
}

/// Runs `asked` on `e` (and the catalog of the store at `store`, for coverage) and writes its
/// body, the places of `page` only: the work of one request.
pub(crate) fn answer(
    e: &Engine,
    store: Option<&Path>,
    asked: &Asked,
    geojson: bool,
    limits: &Limits,
    page: Page,
    out: &mut Vec<u8>,
) -> Result<Answered, QueryError> {
    WORK.with_borrow_mut(|(s, items, codes)| {
        let fail = |x: output::OutputError| match x {
            output::OutputError::Engine(e) => QueryError::from(e),
            output::OutputError::Io(_) => QueryError::new(Code::Internal, "the answer failed"),
        };
        match asked {
            Asked::Places(ask) => {
                query::answer(e, ask, limits, s, items)?;
                items.drain(..page.skip.min(items.len()));
                let more = items.len() > page.take;
                items.truncate(page.take);
                if geojson {
                    output::write(e, items, Mode::Geojson, out).map_err(fail)?;
                } else {
                    out.extend_from_slice(b"{\"results\":");
                    output::write(e, items, Mode::Json, out).map_err(fail)?;
                    close(out, output::attribution_of(e, items).as_deref());
                }
                Ok(Answered {
                    results: items.len(),
                    more,
                })
            }
            Asked::Elevation(lat, lon) => {
                let h = query::elevation(e, *lat, *lon, s)?;
                out.extend_from_slice(b"{\"result\":");
                output::elevation(*lat, *lon, h.as_ref(), true, out).map_err(fail)?;
                close(
                    out,
                    output::attribution_layers(e, h.iter().map(|h| h.layer)).as_deref(),
                );
                Ok(Answered {
                    results: usize::from(h.is_some()),
                    more: false,
                })
            }
            Asked::Postal(ask) => {
                query::postal(e, ask, limits, s, codes)?;
                out.extend_from_slice(b"{\"results\":");
                output::postal(e, codes, true, out).map_err(fail)?;
                close(
                    out,
                    output::attribution_layers(e, codes.iter().map(|c| c.place.layer)).as_deref(),
                );
                Ok(Answered {
                    results: codes.len(),
                    more: false,
                })
            }
            Asked::Boundary(id, lod) => SHAPE.with_borrow_mut(|shape| {
                let got = query::boundary(e, id, *lod, limits, shape)?;
                output::outline(e, &got.place, shape, got.lod, got.outlined, out).map_err(fail)?;
                Ok(Answered {
                    results: 1,
                    more: false,
                })
            }),
            Asked::Distance(from, to) => {
                let m = query::distance(e, from, to, limits)?;
                out.extend_from_slice(b"{\"result\":");
                output::distance(&m, out).map_err(fail)?;
                let layers = [m.from.place, m.to.place];
                close(
                    out,
                    output::attribution_layers(e, layers.iter().flatten().map(|p| p.layer))
                        .as_deref(),
                );
                Ok(Answered {
                    results: 1,
                    more: false,
                })
            }
            Asked::Coverage(lat, lon, radius_m) => {
                coverage(e, store, *lat, *lon, *radius_m, limits, out)
            }
        }
    })
}

/// The countries at or near a point, each with the stored catalog's packs that could hold it
/// (D-318), as `{"results": [...], "attribution": ...}`.
#[cfg(feature = "store")]
fn coverage(
    e: &Engine,
    store: Option<&Path>,
    lat: f64,
    lon: f64,
    radius_m: Option<u32>,
    limits: &Limits,
    out: &mut Vec<u8>,
) -> Result<Answered, QueryError> {
    let at = query::point::<teigeodata_core::E7>(lat, lon)?;
    let radius_m = radius_m.unwrap_or(teigeodata_engine::LOCATOR_RADIUS_M);
    if f64::from(radius_m) > limits.radius_m {
        return Err(QueryError::new(
            Code::OutOfBounds,
            format!("`radius_m` is past the most allowed, {}", limits.radius_m),
        ));
    }
    let root = store.ok_or_else(no_catalog)?;
    // The store's own errors name its paths: a server says only that it cannot read it.
    let (trusted, installed) = teigeodata_sync::client::catalog_at(root)
        .map_err(|_| QueryError::new(Code::Unavailable, "the store's catalog cannot be read"))?;
    let (catalog, _) = teigeodata_sync::catalog::Catalog::trusted(&trusted);
    let (rows, countries) = crate::catalog::coverage(e, &catalog, &installed, at, radius_m)?;
    let results = rows.len();
    out.extend_from_slice(b"{\"results\":");
    serde_json::to_writer(&mut *out, &rows)
        .map_err(|_| QueryError::new(Code::Internal, "the answer failed"))?;
    close(
        out,
        output::attribution_layers(e, countries.iter().map(|c| c.layer)).as_deref(),
    );
    Ok(Answered {
        results,
        more: false,
    })
}

/// Without the `store` feature there is no catalog to read.
#[cfg(not(feature = "store"))]
fn coverage(
    _: &Engine,
    _: Option<&Path>,
    _: f64,
    _: f64,
    _: Option<u32>,
    _: &Limits,
    _: &mut Vec<u8>,
) -> Result<Answered, QueryError> {
    Err(no_catalog())
}

fn no_catalog() -> QueryError {
    QueryError::new(
        Code::NotFound,
        "coverage reads a store's catalog: this server serves packs without one",
    )
}
