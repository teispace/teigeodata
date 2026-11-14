//! The body of an answer, the same for every front end: the native envelope
//! `{"results": [...], "attribution": ...}` (D-032) or GeoJSON, written by the one code path
//! the HTTP routes, the MCP tools and the fuzz harness share. No runtime.

use std::cell::RefCell;

use teigeodata_engine::{Engine, PostalCode, Scratch};

use crate::output::{self, Item, Mode};
use crate::query::{self, Ask, Code, Limits, PostalAsk, QueryError};
use crate::seams::Operation;

/// What a request asks: places, a height or postal codes.
pub(crate) enum Asked {
    Places(Ask),
    Elevation(f64, f64),
    Postal(PostalAsk),
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

/// Runs `asked` on `e` and writes its body, the places of `page` only: the work of one
/// request.
pub(crate) fn answer(
    e: &Engine,
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
        }
    })
}
