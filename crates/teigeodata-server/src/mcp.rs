//! The MCP server (phase 6.3, feature `mcp`, [D-315]): the queries of [`crate::query`] as
//! Model Context Protocol tools and resources, over rmcp (ADR-0028). It speaks revision
//! 2026-07-28 (`server/discover`, no handshake) and serves 2025-11-25 clients and older ones
//! through their `initialize`.
//!
//! **Tools** (read-only, [`TOOLS`]): `search`, `reverse`, `nearby`, `get_place`, `lookup`,
//! `admin_at`, `hierarchy`, `children`, `timezone`, `elevation`, `postal`, `boundary`,
//! `distance` and `packs` (D-317). Each has
//! an `inputSchema` with the server's bounds and an `outputSchema`, and answers
//! `structuredContent` with the same object as text: the HTTP server's native body,
//! `{"results": [...], "attribution": ...}` (D-032), from the same code. The attribution is in
//! `_meta` too (`teigeodata/attribution`). A failure is a tool error (`isError`) whose text is
//! `{"error": {"code", "message"}}` with the stable [`Code`] names.
//!
//! **Pages.** `search`, `reverse`, `nearby` and `children` answer `limit` places and, when more
//! are there, a `next_cursor`: the call and its offset, signed with a key the server makes at
//! start (BLAKE3 keyed), so paging keeps the protocol stateless and a cursor cannot be forged
//! or carried to another tool. Pages reach [`DEPTH`] places at most; a cursor from an earlier
//! run of the server is refused.
//!
//! **Resources.** `teigeodata://dataset/{address}/{component}` (a pack: release, places,
//! components, attribution), `teigeodata://place/{id}` (a place with every name, as a
//! GeoJSON feature) and `teigeodata://boundary/{id}?lod=` (its outline as a GeoJSON feature).
//! The `boundary` tool answers what the outline is and links that resource: an outline runs to
//! thousands of vertices, too many for a model's context.
//!
//! The engine's calls run inline (D-309) and every call passes the seams (ADR-0025); a panic
//! answers `internal` and the server goes on.
//!
//! [D-315]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/log.md

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
    Implementation, JsonObject, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    MetaObject, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ResourceTemplate, ServerCapabilities,
    ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{MaybeSendFuture, RequestContext};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};

use crate::body::{self, Asked, Page, WORK};
use crate::output::{self, Bufs};
use crate::packs::Packs;
use crate::query::{self, Ask, At, Code, Limits, PostalAsk, QueryError};
use crate::seams::{Bare, Caller, Incoming, Operation, Seams, Usage};

/// The deepest place a cursor reaches: five pages of the most results [`Limits::SERVER`]
/// allows.
pub const DEPTH: usize = 250;

/// The `_meta` key of an answer's attribution.
pub const ATTRIBUTION: &str = "teigeodata/attribution";

/// The tools, in the order `tools/list` gives them.
pub const TOOLS: [&str; 15] = [
    "search",
    "reverse",
    "nearby",
    "get_place",
    "lookup",
    "admin_at",
    "hierarchy",
    "children",
    "timezone",
    "elevation",
    "postal",
    "boundary",
    "distance",
    "packs",
    "coverage",
];

/// The tools that read the store's catalog: listed only on a server over a store (D-318).
pub const STORE_TOOLS: [&str; 1] = ["coverage"];

/// The longest cursor accepted, in bytes: a signed call with arguments at their bounds is
/// far shorter.
const CURSOR_BYTES: usize = 4096;

/// How long a client may keep `tools/list` (it changes only with the server).
const TOOLS_TTL_MS: u64 = 3_600_000;

/// How long a client may keep a pack list or a resource read (the store may change).
const DATA_TTL_MS: u64 = 60_000;

const DATASET: &str = "teigeodata://dataset/";
const PLACE: &str = "teigeodata://place/";
const BOUNDARY: &str = "teigeodata://boundary/";

/// The MCP server over a pack set: clone it freely (it is one `Arc`).
#[derive(Clone)]
pub struct Mcp(Arc<Inner>);

struct Inner {
    packs: Arc<Packs>,
    seams: Seams,
    limits: Limits,
    key: [u8; 32],
    tools: Vec<Tool>,
}

impl core::fmt::Debug for Mcp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Mcp")
            .field("limits", &self.0.limits)
            .finish_non_exhaustive()
    }
}

impl Mcp {
    /// A server over `packs`, admitting calls through `seams`, within `limits`, with a cursor
    /// key from the operating system's random source.
    ///
    /// # Errors
    /// The random source failed.
    pub fn new(packs: Arc<Packs>, seams: Seams, limits: Limits) -> std::io::Result<Self> {
        let store = packs.root().is_some();
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).map_err(std::io::Error::other)?;
        Ok(Self(Arc::new(Inner {
            packs,
            seams,
            limits,
            key,
            tools: tools(&limits)
                .into_iter()
                .filter(|t| store || !STORE_TOOLS.contains(&t.name.as_ref()))
                .collect(),
        })))
    }

    /// Runs the tool `name` with `args` for `incoming` (what the seams read): what
    /// `tools/call` does, without a transport. `None` for no such tool.
    #[must_use]
    pub fn call(
        &self,
        name: &str,
        args: &Map<String, Value>,
        incoming: &dyn Incoming,
    ) -> Option<CallToolResult> {
        if !self.0.tools.iter().any(|t| t.name == name) {
            return None;
        }
        let tool = TOOLS.iter().position(|t| *t == name)?;
        let start = Instant::now();
        // The engine's calls are synchronous: a panic is caught here, answered, and the
        // server goes on.
        let done = catch_unwind(AssertUnwindSafe(|| self.run(tool, args, incoming, start)))
            .unwrap_or_else(|_| Err(QueryError::new(Code::Internal, "the server failed")));
        Some(match done {
            Ok(r) => r,
            Err(e) => failed(&e),
        })
    }

    fn run(
        &self,
        tool: usize,
        given: &Map<String, Value>,
        incoming: &dyn Incoming,
        start: Instant,
    ) -> Result<CallToolResult, QueryError> {
        let name = TOOLS.get(tool).copied().unwrap_or_default();
        let (args, offset) = match given.get("cursor") {
            None | Some(Value::Null) => (given.clone(), 0),
            Some(Value::String(c)) => self.open_cursor(name, c)?,
            Some(_) => return Err(bad("`cursor` must be a string")),
        };
        let a = Args(&args);
        if name == "packs" {
            self.0.seams.admit(incoming, Operation::Packs)?;
            let e = self.0.packs.load();
            return Ok(done(output::packs(&e), output::attribution(&e)));
        }
        if name == "boundary" {
            let caller = self.0.seams.admit(incoming, Operation::Boundary)?;
            let r = self.outline_summary(&a)?;
            return Ok(self.metered(&caller, Operation::Boundary, 1, r, start));
        }
        let asked = asked(name, &a, &self.0.limits)?;
        let op = asked.operation();
        let caller = self.0.seams.admit(incoming, op)?;
        // A paged ask runs to the end of its page and one more, so the answer knows whether
        // more are there; its own `limit` is checked first, at the server's bound.
        let (asked, page, limits) = match asked {
            Asked::Places(ask) => match deeper(&ask, offset) {
                Some((deep, take)) => (
                    Asked::Places(deep),
                    Page { skip: offset, take },
                    Limits {
                        limit: DEPTH + 1,
                        ..self.0.limits
                    },
                ),
                None => (Asked::Places(ask), Page::ALL, self.0.limits),
            },
            other => (other, Page::ALL, self.0.limits),
        };
        let e = self.0.packs.load();
        let mut text = Vec::with_capacity(1024);
        let store = self.0.packs.root();
        let answered = body::answer(&e, store, &asked, false, &limits, page, &mut text)?;
        let mut v: Value = serde_json::from_slice(&text)
            .map_err(|_| QueryError::new(Code::Internal, "the answer failed"))?;
        let attribution = v
            .get("attribution")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let next = page.skip.saturating_add(page.take);
        if answered.more
            && next < DEPTH
            && let Some(o) = v.as_object_mut()
        {
            o.insert("next_cursor".into(), self.cursor(name, &args, next).into());
        }
        Ok(self.metered(&caller, op, answered.results, done(v, attribution), start))
    }

    /// Records a call's usage with the meter seam, and passes its result on.
    fn metered(
        &self,
        caller: &Caller,
        op: Operation,
        results: usize,
        r: CallToolResult,
        start: Instant,
    ) -> CallToolResult {
        let bytes = r
            .content
            .first()
            .and_then(|c| c.as_text())
            .map_or(0, |t| t.text.len());
        self.0.seams.meter.record(
            caller,
            op,
            Usage {
                results,
                bytes,
                micros: u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
            },
        );
        r
    }

    /// A cursor for `tool` called with `args` (less any cursor), at `offset`: the call as
    /// JSON and its keyed hash, in hex.
    fn cursor(&self, tool: &str, args: &Map<String, Value>, offset: usize) -> String {
        let mut a = args.clone();
        a.remove("cursor");
        let payload = json!({ "t": tool, "a": a, "o": offset }).to_string();
        let mac = blake3::keyed_hash(&self.0.key, payload.as_bytes());
        let mut out = String::with_capacity((payload.len() + 32) * 2);
        for b in payload.as_bytes().iter().chain(mac.as_bytes()) {
            out.push(hex(b >> 4));
            out.push(hex(b & 15));
        }
        out
    }

    /// The call and offset in `cursor`, if this server signed it for `tool`.
    fn open_cursor(
        &self,
        tool: &str,
        cursor: &str,
    ) -> Result<(Map<String, Value>, usize), QueryError> {
        let refused =
            || bad("the cursor is not one this server gave for this tool: ask again without it");
        if cursor.len() > CURSOR_BYTES || !cursor.len().is_multiple_of(2) || cursor.len() < 66 {
            return Err(refused());
        }
        let bytes: Vec<u8> = cursor
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[hi, lo]| Some(unhex(hi)? << 4 | unhex(lo)?))
            .collect::<Option<_>>()
            .ok_or_else(refused)?;
        let (payload, mac) = bytes.split_at(bytes.len() - 32);
        let mac: [u8; 32] = mac.try_into().map_err(|_| refused())?;
        // `Hash` compares in constant time.
        if blake3::keyed_hash(&self.0.key, payload) != blake3::Hash::from_bytes(mac) {
            return Err(refused());
        }
        let v: Value = serde_json::from_slice(payload).map_err(|_| refused())?;
        match (v.get("t"), v.get("a"), v.get("o").and_then(Value::as_u64)) {
            (Some(Value::String(t)), Some(Value::Object(a)), Some(o)) if t == tool => {
                Ok((a.clone(), usize::try_from(o).map_err(|_| refused())?))
            }
            _ => Err(refused()),
        }
    }

    /// The `boundary` tool: what the outline is (level of detail, vertices, parts, box) and a
    /// link to it as a resource, not the vertices themselves: an outline is too large for a
    /// model's context and a client fetches it when it draws.
    fn outline_summary(&self, args: &Args<'_>) -> Result<CallToolResult, QueryError> {
        let id = args.need_text("id")?;
        let lod = args
            .whole("lod")?
            .map(|l| u8::try_from(l).unwrap_or(u8::MAX));
        let e = self.0.packs.load();
        body::SHAPE.with_borrow_mut(|shape| {
            let got = query::boundary(&e, &id, lod, &self.0.limits, shape)?;
            let mut buf = [0_u8; output::NAME];
            let name = output::installed(e.name(&got.place, &mut buf))
                .map_err(QueryError::from)?
                .map(str::to_owned);
            let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for point in &shape.points {
                let (lat, lon) = (point.lat_degrees(), point.lon_degrees());
                bbox = [
                    bbox[0].min(lon),
                    bbox[1].min(lat),
                    bbox[2].max(lon),
                    bbox[3].max(lat),
                ];
            }
            let uri = format!("{BOUNDARY}{}?lod={}", got.place.id, got.lod);
            let result = if got.outlined {
                json!({
                    "id": got.place.id.to_string(), "name": name, "outlined": true,
                    "lod": got.lod, "tolerance_m": f64::from(shape.tolerance_cm) / 100.0,
                    "vertices": shape.points.len(),
                    "parts": shape.rings.iter().map(|r| r.part).max().map_or(0, |p| p + 1),
                    "holes": shape.rings.iter().filter(|r| r.hole).count(),
                    "bbox": bbox, "uri": uri,
                })
            } else {
                json!({
                    "id": got.place.id.to_string(), "name": name, "outlined": false,
                    "lod": got.lod, "tolerance_m": 0.0, "vertices": 1, "parts": 0, "holes": 0,
                    "bbox": null, "uri": uri,
                })
            };
            let attribution = output::attribution_layers(&e, [got.place.layer]);
            let mut r = done(
                json!({ "result": result, "attribution": attribution }),
                attribution.clone(),
            );
            r.content.push(ContentBlock::resource_link(
                Resource::new(uri, format!("boundary of {}", got.place.id))
                    .with_mime_type("application/geo+json"),
            ));
            Ok(r)
        })
    }

    /// Every pack as a dataset resource.
    fn datasets(&self, incoming: &dyn Incoming) -> Result<ListResourcesResult, QueryError> {
        self.0.seams.admit(incoming, Operation::Packs)?;
        let e = self.0.packs.load();
        let items = e
            .layers()
            .map(|l| {
                let name = format!("{}/{}", l.address(), l.component());
                Resource::new(format!("{DATASET}{name}"), name.clone())
                    .with_title(format!("Pack {name} {}", l.release()))
                    .with_description(format!(
                        "{} places; release {}; attribution: {}",
                        l.len(),
                        l.release(),
                        l.attribution().unwrap_or("none stated")
                    ))
                    .with_mime_type("application/json")
            })
            .collect();
        Ok(ListResourcesResult::with_all_items(items)
            .with_ttl_ms(DATA_TTL_MS)
            .with_cache_scope(CacheScope::Private))
    }

    fn read(&self, uri: &str, incoming: &dyn Incoming) -> Result<ResourceContents, QueryError> {
        let e = self.0.packs.load();
        if let Some(rest) = uri.strip_prefix(DATASET) {
            self.0.seams.admit(incoming, Operation::Packs)?;
            let (address, component) = rest.split_once('/').unwrap_or((rest, ""));
            let l = e
                .layers()
                .find(|l| l.address() == address && l.component() == component)
                .ok_or_else(|| QueryError::new(Code::NotFound, format!("{uri}: not found")))?;
            return Ok(ResourceContents::text(output::pack(l).to_string(), uri)
                .with_mime_type("application/json"));
        }
        if let Some(rest) = uri.strip_prefix(BOUNDARY) {
            self.0.seams.admit(incoming, Operation::Boundary)?;
            let (id, query) = rest.split_once('?').unwrap_or((rest, ""));
            let id = percent_decoded(id).ok_or_else(|| bad("the place ID is not valid UTF-8"))?;
            let lod = match query.strip_prefix("lod=") {
                None if query.is_empty() => None,
                Some(l) => Some(l.parse::<u8>().unwrap_or(u8::MAX)),
                None => return Err(bad("the only parameter is `lod`")),
            };
            let mut text = Vec::with_capacity(4096);
            body::answer(
                &e,
                None,
                &Asked::Boundary(id, lod),
                true,
                &self.0.limits,
                Page::ALL,
                &mut text,
            )?;
            let text = String::from_utf8(text)
                .map_err(|_| QueryError::new(Code::Internal, "the answer failed"))?;
            return Ok(ResourceContents::text(text, uri).with_mime_type("application/geo+json"));
        }
        let Some(id) = uri.strip_prefix(PLACE) else {
            return Err(QueryError::new(Code::NotFound, format!("{uri}: not found")));
        };
        self.0.seams.admit(incoming, Operation::Get)?;
        let id = percent_decoded(id).ok_or_else(|| bad("the place ID is not valid UTF-8"))?;
        let item = WORK.with_borrow_mut(|(sc, items, _)| {
            let get = Ask::Get { id, names: true };
            query::answer(&e, &get, &self.0.limits, sc, items)?;
            items
                .first()
                .copied()
                .ok_or_else(|| QueryError::new(Code::Internal, "the answer failed"))
        })?;
        let mut bufs = Bufs::default();
        let r = output::record(&e, &item, &mut bufs, false)?;
        let mut f = Vec::with_capacity(1024);
        output::feature(&mut f, &item.place, r)
            .map_err(|_| QueryError::new(Code::Internal, "the answer failed"))?;
        let mut v: Value = serde_json::from_slice(&f)
            .map_err(|_| QueryError::new(Code::Internal, "the answer failed"))?;
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "attribution".into(),
                output::attribution_of(&e, &[item]).into(),
            );
        }
        Ok(ResourceContents::text(v.to_string(), uri).with_mime_type("application/geo+json"))
    }
}

/// Serves `server` on standard input and output until the client closes them, following the
/// store's journal every `follow_every` (D-172).
///
/// # Errors
/// The transport's, as I/O errors.
pub async fn serve_stdio(server: Mcp, follow_every: Option<Duration>) -> std::io::Result<()> {
    if let Some(every) = follow_every {
        crate::packs::spawn_follow(Arc::clone(&server.0.packs), every);
    }
    let running = match server.serve(rmcp::transport::stdio()).await {
        Ok(r) => r,
        // The client left before it began a session (after `server/discover` alone, say):
        // the end of the conversation, not a failure.
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
        Err(e) => return Err(std::io::Error::other(e)),
    };
    running.waiting().await.map_err(std::io::Error::other)?;
    Ok(())
}

// rmcp's handler methods are futures; ours are ready at once (the engine's calls run inline,
// D-309).
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new("teigeodata", env!("CARGO_PKG_VERSION"))
                .with_title("teigeodata")
                .with_description(
                    "Places offline: search, reverse geocoding, admin units, timezones, \
                     elevation and postal codes from signed packs",
                ),
        )
        .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(self.0.tools.clone())
            .with_ttl_ms(TOOLS_TTL_MS)
            .with_cache_scope(CacheScope::Public)))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.0.tools.iter().find(|t| t.name == name).cloned()
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, ErrorData>> + MaybeSendFuture + '_ {
        let empty = JsonObject::new();
        let args = request.arguments.as_ref().unwrap_or(&empty);
        std::future::ready(
            incoming(&context, |i| self.call(&request.name, args, i))
                .map(CallToolResponse::from)
                .ok_or_else(|| {
                    ErrorData::invalid_params(format!("no tool {:?}", request.name), None)
                }),
        )
    }

    fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(incoming(&context, |i| self.datasets(i)).map_err(protocol))
    }

    fn list_resource_templates(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + MaybeSendFuture + '_
    {
        let items = vec![
            ResourceTemplate::new(format!("{PLACE}{{id}}"), "place")
                .with_title("A place")
                .with_description(
                    "A place by its ID (tgd:np/1xhb), with every name, as a GeoJSON feature",
                )
                .with_mime_type("application/geo+json"),
            ResourceTemplate::new(format!("{BOUNDARY}{{id}}{{?lod}}"), "boundary")
                .with_title("A place's outline")
                .with_description(
                    "A place's outline as a GeoJSON feature, simplified to level of detail `lod` \
                     (0 finest to 6 coarsest)",
                )
                .with_mime_type("application/geo+json"),
            ResourceTemplate::new(format!("{DATASET}{{address}}/{{component}}"), "dataset")
                .with_title("A pack")
                .with_description(
                    "A pack (np/places): its release, places, installed components and \
                     attribution",
                )
                .with_mime_type("application/json"),
        ];
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(items)
            .with_ttl_ms(TOOLS_TTL_MS)
            .with_cache_scope(CacheScope::Public)))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, ErrorData>> + MaybeSendFuture + '_ {
        let read = catch_unwind(AssertUnwindSafe(|| {
            incoming(&context, |i| self.read(&request.uri, i))
        }))
        .unwrap_or_else(|_| Err(QueryError::new(Code::Internal, "the server failed")));
        std::future::ready(read.map_err(protocol).map(|contents| {
            ReadResourceResult::new(vec![contents])
                .with_ttl_ms(DATA_TTL_MS)
                .with_cache_scope(CacheScope::Private)
                .into()
        }))
    }
}

/// Runs `f` with what the request carries for the seams: over HTTP its headers and peer, on
/// stdio nothing.
fn incoming<R>(context: &RequestContext<RoleServer>, f: impl FnOnce(&dyn Incoming) -> R) -> R {
    #[cfg(feature = "http")]
    if let Some(parts) = context.extensions.get::<axum::http::request::Parts>() {
        let req = crate::http::Request {
            headers: &parts.headers,
            peer: parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|c| c.0),
        };
        return f(&req);
    }
    #[cfg(not(feature = "http"))]
    let _ = context;
    f(&Bare)
}

/// The MCP server as stateless Streamable HTTP (D-316), for the HTTP server's `/mcp`: JSON
/// answers, no sessions, a `Host` in `hosts` (none: any), and a browser's `Origin` refused
/// unless it is one of `origins`.
#[cfg(feature = "http")]
pub(crate) fn http_service(
    server: Mcp,
    hosts: &[String],
    origins: &[axum::http::HeaderValue],
) -> rmcp::transport::streamable_http_server::StreamableHttpService<
    Mcp,
    rmcp::transport::streamable_http_server::session::never::NeverSessionManager,
> {
    use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;

    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_sse_retry(None)
        .with_max_request_body_bytes(MAX_BODY_BYTES);
    config = if hosts.is_empty() {
        config.disable_allowed_hosts()
    } else {
        config.with_allowed_hosts(hosts.iter().cloned())
    };
    // With no origins, every `Origin` sent is refused (a native client sends none).
    let origins: Vec<String> = origins
        .iter()
        .filter_map(|o| o.to_str().ok())
        .map(with_port)
        .collect();
    config = if origins.is_empty() {
        config.enforce_origin_validation()
    } else {
        config.with_allowed_origins(origins)
    };
    rmcp::transport::streamable_http_server::StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::default(),
        config,
    )
}

/// The longest `/mcp` request body: a call at every bound is a few kilobytes.
#[cfg(feature = "http")]
const MAX_BODY_BYTES: usize = 64 * 1024;

/// `origin` with its scheme's port when it names none (`https://a.example` matches port 443
/// only, never any port).
#[cfg(feature = "http")]
fn with_port(origin: &str) -> String {
    let after_scheme = origin.split_once("://").map_or("", |(_, rest)| rest);
    // An IPv6 host is bracketed: a port follows the bracket.
    let host_end = after_scheme.rfind(']').map_or(0, |i| i + 1);
    if after_scheme
        .get(host_end..)
        .is_some_and(|r| r.contains(':'))
    {
        return origin.to_owned();
    }
    match origin.split_once("://").map(|(s, _)| s) {
        Some("https") => format!("{origin}:443"),
        Some("http") => format!("{origin}:80"),
        _ => origin.to_owned(),
    }
}

const INSTRUCTIONS: &str = "Offline place data. Find places by name with `search`, by \
coordinates with `reverse` and `admin_at`, around a place with `nearby`; climb with \
`hierarchy` and descend with `children`. Places are named by IDs such as tgd:np/1xhb: pass \
them to `get_place`, `lookup`, `nearby`, `timezone` or read teigeodata://place/{id}. \
Coordinates are WGS84 degrees, distances metres, heights metres above EGM2008. A long answer \
has a `next_cursor`: call the same tool with only `cursor` for the next page. Show the \
`attribution` of an answer wherever its data is shown: the licences require it.";

/// A tool error: the stable code and the message, as the HTTP server answers them.
fn failed(e: &QueryError) -> CallToolResult {
    let text = json!({ "error": { "code": e.code.name(), "message": e.message } }).to_string();
    CallToolResult::error(vec![ContentBlock::text(text)])
}

/// A protocol error for a resource or list request: not found or a bad argument is
/// `-32602`, as 2026-07-28 has it; a refusal by a seam or a failure, an internal error with
/// its code.
fn protocol(e: QueryError) -> ErrorData {
    let data = Some(json!({ "code": e.code.name() }));
    match e.code {
        Code::NotFound | Code::InvalidArgument | Code::OutOfBounds => {
            ErrorData::invalid_params(e.message, data)
        }
        _ => ErrorData::internal_error(e.message, data),
    }
}

/// A finished answer: `v` as structured content and as text, `attribution` in `_meta`.
fn done(v: Value, attribution: Option<String>) -> CallToolResult {
    let mut r = CallToolResult::structured(v);
    if let Some(a) = attribution {
        let mut m = Map::new();
        m.insert(ATTRIBUTION.into(), a.into());
        r = r.with_meta(Some(MetaObject(m)));
    }
    r
}

fn bad(message: &str) -> QueryError {
    QueryError::new(Code::InvalidArgument, message)
}

/// `ask` run deeper for the page at `offset`: the ask with room for the page and one more,
/// and the page's size. `None` for an ask that is not paged.
fn deeper(ask: &Ask, offset: usize) -> Option<(Ask, usize)> {
    let room = |limit: usize| {
        offset
            .saturating_add(limit)
            .saturating_add(1)
            .min(DEPTH + 1)
    };
    let mut deep = ask.clone();
    let take = match &mut deep {
        Ask::Search { limit, .. }
        | Ask::Reverse { limit, .. }
        | Ask::Near { limit, .. }
        | Ask::Children { limit, .. } => {
            let take = *limit;
            *limit = room(take);
            take
        }
        _ => return None,
    };
    Some((deep, take))
}

/// The arguments of a call.
struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name).filter(|v| !v.is_null())
    }

    fn text(&self, name: &str) -> Result<Option<String>, QueryError> {
        match self.get(name) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(bad(&format!("`{name}` must be a string"))),
        }
    }

    fn need_text(&self, name: &str) -> Result<String, QueryError> {
        self.text(name)?
            .ok_or_else(|| bad(&format!("`{name}` is required")))
    }

    fn num(&self, name: &str) -> Result<Option<f64>, QueryError> {
        match self.get(name) {
            None => Ok(None),
            Some(v) => v
                .as_f64()
                .filter(|n| n.is_finite())
                .map(Some)
                .ok_or_else(|| bad(&format!("`{name}` must be a number"))),
        }
    }

    fn need(&self, name: &str) -> Result<f64, QueryError> {
        self.num(name)?
            .ok_or_else(|| bad(&format!("`{name}` is required")))
    }

    fn flag(&self, name: &str) -> Result<bool, QueryError> {
        match self.get(name) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(bad(&format!("`{name}` must be true or false"))),
        }
    }

    fn whole(&self, name: &str) -> Result<Option<u64>, QueryError> {
        match self.get(name) {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| bad(&format!("`{name}` must be a whole number, 0 or more"))),
        }
    }

    fn limit(&self) -> Result<usize, QueryError> {
        Ok(self.whole("limit")?.map_or(Limits::DEFAULT_LIMIT, |n| {
            usize::try_from(n).unwrap_or(usize::MAX)
        }))
    }
}

/// The ask of tool `name` with `a`.
fn asked(name: &str, a: &Args<'_>, limits: &Limits) -> Result<Asked, QueryError> {
    if let Some(other) = not_places(name, a)? {
        return Ok(other);
    }
    let ask = match name {
        "search" => Ask::Search {
            query: a.need_text("query")?,
            limit: a.limit()?,
            focus: match a.get("focus") {
                None => None,
                Some(Value::Object(f)) => {
                    let f = Args(f);
                    Some((
                        f.need("lat")?,
                        f.need("lon")?,
                        f.num("scale_km")?.unwrap_or(50.0) * 1_000.0,
                    ))
                }
                Some(_) => return Err(bad("`focus` must be an object: {lat, lon, scale_km}")),
            },
            strict: a.flag("strict")?,
        },
        "reverse" => Ask::Reverse {
            lat: a.need("lat")?,
            lon: a.need("lon")?,
            limit: a.limit()?,
            in_country: a.flag("in_country")?,
        },
        "nearby" => Ask::Near {
            id: a.need_text("id")?,
            limit: a.limit()?,
            radius_m: a.num("radius_m")?,
        },
        "get_place" => Ask::Get {
            id: a.need_text("id")?,
            names: a.flag("names")?,
        },
        "lookup" => match a.get("ids") {
            Some(Value::Array(ids)) => Ask::Lookup {
                ids: ids
                    .iter()
                    .map(|i| i.as_str().map(str::to_owned))
                    .collect::<Option<_>>()
                    .ok_or_else(|| bad("`ids` must be strings"))?,
            },
            Some(_) => return Err(bad("`ids` must be an array of strings")),
            None => return Err(bad("`ids` is required")),
        },
        "admin_at" => Ask::Admin {
            lat: a.need("lat")?,
            lon: a.need("lon")?,
        },
        "hierarchy" => Ask::Hierarchy {
            id: a.need_text("id")?,
        },
        "children" => Ask::Children {
            id: a.need_text("id")?,
            limit: a.limit()?,
        },
        "timezone" => {
            let t = match a.get("t") {
                None => body::now(),
                Some(v) => v
                    .as_i64()
                    .ok_or_else(|| bad("`t` must be whole Unix seconds"))?,
            };
            match a.text("id")? {
                Some(id) => Ask::TzOf { id, t },
                None => Ask::TzAt {
                    lat: a.need("lat")?,
                    lon: a.need("lon")?,
                    t,
                },
            }
        }
        _ => return Err(QueryError::new(Code::NotFound, "no such tool")),
    };
    // The bounds hold before any page is computed: `limit` at the server's own.
    ask.check(limits)?;
    Ok(Asked::Places(ask))
}

/// The asks of the tools that answer no list of places, or `None` for a place tool.
fn not_places(name: &str, a: &Args<'_>) -> Result<Option<Asked>, QueryError> {
    Ok(Some(match name {
        "elevation" => Asked::Elevation(a.need("lat")?, a.need("lon")?),
        "coverage" => Asked::Coverage(
            a.need("lat")?,
            a.need("lon")?,
            a.whole("radius_m")?
                .map(|r| u32::try_from(r).unwrap_or(u32::MAX)),
        ),
        "distance" => Asked::Distance(
            At::parse(&a.need_text("from")?),
            At::parse(&a.need_text("to")?),
        ),
        "postal" => Asked::Postal(match a.text("code")? {
            Some(code) => PostalAsk::Code {
                code,
                scheme: a.text("scheme")?,
                prefix: a.flag("prefix")?,
                limit: a.limit()?,
            },
            None => PostalAsk::At {
                lat: a.need("lat")?,
                lon: a.need("lon")?,
            },
        }),
        _ => return Ok(None),
    }))
}

fn hex(n: u8) -> char {
    char::from(if n < 10 { b'0' + n } else { b'a' + n - 10 })
}

fn unhex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// `s` with `%XX` escapes decoded (a URI template may escape the ID's `:` and `/`).
fn percent_decoded(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hi = b
                .get(i + 1)
                .copied()
                .and_then(|h| unhex(h.to_ascii_lowercase()))?;
            let lo = b
                .get(i + 2)
                .copied()
                .and_then(|l| unhex(l.to_ascii_lowercase()))?;
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The tools' definitions, their bounds from `limits`.
#[allow(clippy::too_many_lines, reason = "one table: the tools' definitions")]
fn tools(limits: &Limits) -> Vec<Tool> {
    let lat = json!({ "type": "number", "minimum": -90, "maximum": 90, "description": "Latitude, WGS84 degrees." });
    let lon = json!({ "type": "number", "minimum": -180, "maximum": 180, "description": "Longitude, WGS84 degrees." });
    let id = json!({ "type": "string", "minLength": 1, "maxLength": limits.text_bytes, "description": "A place ID from an earlier answer, such as tgd:np/1xhb." });
    let limit = json!({ "type": "integer", "minimum": 0, "maximum": limits.limit, "default": Limits::DEFAULT_LIMIT, "description": "How many results, the page's size." });
    let cursor = json!({ "type": "string", "description": "The `next_cursor` of the previous page; the other arguments are then taken from it." });
    let places = places_schema();
    let read_only = || {
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false)
    };
    let def = |name: &'static str,
               title: &str,
               description: &str,
               props: Value,
               required: &[&str],
               output: &Value| {
        let mut input = Map::new();
        input.insert("type".into(), "object".into());
        input.insert("properties".into(), props);
        if !required.is_empty() {
            input.insert("required".into(), json!(required));
        }
        let Value::Object(out) = output.clone() else {
            unreachable!("a schema is an object")
        };
        Tool::new_with_raw(name, Some(description.to_owned().into()), input)
            .with_title(title)
            .with_raw_output_schema(Arc::new(out))
            .with_annotations(read_only())
    };
    vec![
        def(
            "search",
            "Search places by name",
            "Places matching a name, best first. \"Name, Parent\" ranks places under a parent of that name first. `focus` ranks places near a point higher (scale_km: how far counts as near, default 50). `strict`: every word, in order.",
            json!({
                "query": { "type": "string", "minLength": 1, "maxLength": limits.text_bytes, "description": "The name, in any script, such as Pokhara or पोखरा." },
                "limit": limit, "cursor": cursor, "strict": { "type": "boolean", "default": false },
                "focus": { "type": "object", "properties": { "lat": lat, "lon": lon, "scale_km": { "type": "number", "exclusiveMinimum": 0, "default": 50 } }, "required": ["lat", "lon"] },
            }),
            &["query"],
            &places,
        ),
        def(
            "reverse",
            "Places nearest a point",
            "The places nearest a point, nearest first, each with its distance in metres, its bearing, and whether its outline holds the point. `in_country`: only places in the country holding the point.",
            json!({ "lat": lat, "lon": lon, "limit": limit, "cursor": cursor, "in_country": { "type": "boolean", "default": false } }),
            &["lat", "lon"],
            &places,
        ),
        def(
            "nearby",
            "Places near a place",
            "The places nearest a place, nearest first, the place itself left out; within `radius_m` metres when given.",
            json!({ "id": id, "limit": limit, "cursor": cursor, "radius_m": { "type": "number", "exclusiveMinimum": 0, "maximum": limits.radius_m, "description": "Metres." } }),
            &["id"],
            &places,
        ),
        def(
            "get_place",
            "A place by ID",
            "One place by its ID; `names`: every name it has, with language and origin. A retired ID answers its survivor.",
            json!({ "id": id, "names": { "type": "boolean", "default": false } }),
            &["id"],
            &places,
        ),
        def(
            "lookup",
            "Places by IDs",
            "Places by ID, in the order asked; IDs not found are left out.",
            json!({ "ids": { "type": "array", "items": id, "minItems": 1, "maxItems": limits.limit } }),
            &["ids"],
            &places,
        ),
        def(
            "admin_at",
            "Admin units at a point",
            "The admin units holding a point, the country first, then each finer level.",
            json!({ "lat": lat, "lon": lon }),
            &["lat", "lon"],
            &places,
        ),
        def(
            "hierarchy",
            "Ancestors of a place",
            "A place's ancestors: its parent first, then the parent's, up to the country.",
            json!({ "id": id }),
            &["id"],
            &places,
        ),
        def(
            "children",
            "Children of a place",
            "The places whose parent is a place, such as a country's provinces or a district's municipalities.",
            json!({ "id": id, "limit": limit, "cursor": cursor }),
            &["id"],
            &places,
        ),
        def(
            "timezone",
            "Timezone",
            "The IANA timezone of a place (`id`) or at a point (`lat`, `lon`), with its UTC offset, abbreviation and DST at instant `t` (Unix seconds, default now).",
            json!({ "id": id, "lat": lat, "lon": lon, "t": { "type": "integer", "description": "Unix seconds." } }),
            &[],
            &places,
        ),
        def(
            "elevation",
            "Terrain height",
            "The terrain height at a point, metres above the EGM2008 geoid, from the packs' elevation model; null where there is none.",
            json!({ "lat": lat, "lon": lon }),
            &["lat", "lon"],
            &elevation_schema(),
        ),
        def(
            "postal",
            "Postal codes",
            "Postal codes: a code to its places (`code`, optionally its `scheme` such as NP or IN-PIN), codes starting with `code` (`prefix`), or the codes at a point (`lat`, `lon`).",
            json!({
                "code": { "type": "string", "minLength": 1, "maxLength": limits.text_bytes },
                "scheme": { "type": "string", "maxLength": limits.text_bytes },
                "prefix": { "type": "boolean", "default": false },
                "limit": limit, "lat": lat, "lon": lon,
            }),
            &[],
            &postal_schema(),
        ),
        def(
            "boundary",
            "Outline of a place",
            "What a place's outline is: its level of detail (0 finest to 6 coarsest; the finest within the vertex bound when `lod` is not given), tolerance in metres, vertices, parts, holes and bounding box, and the `uri` of the outline itself as a GeoJSON resource, to read when it is drawn. A place with no outline answers `outlined: false`.",
            json!({ "id": id, "lod": { "type": "integer", "minimum": 0, "maximum": query::COARSEST_LOD } }),
            &["id"],
            &boundary_schema(),
        ),
        def(
            "distance",
            "Distance and bearing",
            "The great-circle distance in metres and initial bearing between two ends, each a place ID or a point as `lat,lon`.",
            json!({
                "from": { "type": "string", "minLength": 1, "maxLength": limits.text_bytes, "description": "A place ID such as tgd:np/1xhb, or a point such as 27.7172,85.324." },
                "to": { "type": "string", "minLength": 1, "maxLength": limits.text_bytes, "description": "A place ID, or a point as `lat,lon`." },
            }),
            &["from", "to"],
            &distance_schema(),
        ),
        def(
            "packs",
            "Installed packs",
            "The packs served: address, component, release, places, installed components and attribution.",
            json!({}),
            &[],
            &packs_schema(),
        ),
        def(
            "coverage",
            "Packs for a point",
            "The countries at a point or within `radius_m` metres of it, each with the catalog's packs that could hold the point: path, release, download and disk bytes, components, parts, licence and state (installed, update or available). From the store's verified catalog, offline.",
            json!({ "lat": lat, "lon": lon, "radius_m": { "type": "integer", "minimum": 0, "maximum": limits.radius_m, "description": "Metres; default 3,000." } }),
            &["lat", "lon"],
            &coverage_schema(),
        ),
    ]
}

fn nullable(t: &str) -> Value {
    json!([t, "null"])
}

fn places_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "results": { "type": "array", "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": nullable("string") },
                    "kind": { "type": nullable("string") },
                    "admin_level": { "type": "integer" },
                    "lat": { "type": "number" },
                    "lon": { "type": "number" },
                    "elevation": { "type": nullable("integer"), "description": "Metres above EGM2008." },
                    "elevation_source": { "type": "string" },
                    "parent": { "type": nullable("string") },
                    "timezone": { "type": nullable("string") },
                    "iso_code": { "type": "string" },
                    "meters": { "type": "number" },
                    "bearing": { "type": "number", "description": "Degrees clockwise from north." },
                    "inside": { "type": "boolean" },
                    "score": { "type": "integer" },
                    "names": { "type": "array", "items": { "type": "object", "properties": {
                        "text": { "type": "string" }, "lang": { "type": "string" },
                        "kind": { "type": "string" }, "origin": { "type": "string" },
                        "machine": { "type": "boolean" } }, "required": ["text", "origin", "machine"] } },
                    "utc_offset": { "type": "string" },
                    "tz_abbr": { "type": "string" },
                    "is_dst": { "type": "boolean" },
                },
                "required": ["id", "name", "kind", "admin_level", "elevation", "parent", "timezone"],
            } },
            "attribution": { "type": nullable("string") },
            "next_cursor": { "type": "string" },
        },
        "required": ["results", "attribution"],
    })
}

fn elevation_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "result": { "type": "object", "properties": {
                "lat": { "type": "number" }, "lon": { "type": "number" },
                "elevation_m": { "type": nullable("number") },
                "datum": { "type": "string" },
                "source": { "type": nullable("string") },
                "resolution_m": { "type": "number" },
            }, "required": ["lat", "lon", "elevation_m", "datum"] },
            "attribution": { "type": nullable("string") },
        },
        "required": ["result", "attribution"],
    })
}

fn postal_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "results": { "type": "array", "items": { "type": "object", "properties": {
                "scheme": { "type": "string" }, "code": { "type": "string" },
                "kind": { "type": "string" },
                "valid_from": { "type": nullable("integer") },
                "valid_to": { "type": nullable("integer") },
                "replaced": { "type": "boolean" },
                "id": { "type": "string" },
                "name": { "type": nullable("string") },
                "admin_level": { "type": "integer" },
            }, "required": ["scheme", "code", "kind", "replaced", "id", "name", "admin_level"] } },
            "attribution": { "type": nullable("string") },
        },
        "required": ["results", "attribution"],
    })
}

fn boundary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "result": { "type": "object", "properties": {
                "id": { "type": "string" }, "name": { "type": nullable("string") },
                "outlined": { "type": "boolean" },
                "lod": { "type": "integer" }, "tolerance_m": { "type": "number" },
                "vertices": { "type": "integer" }, "parts": { "type": "integer" },
                "holes": { "type": "integer" },
                "bbox": { "type": ["array", "null"], "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "West, south, east, north." },
                "uri": { "type": "string" },
            }, "required": ["id", "name", "outlined", "lod", "vertices", "bbox", "uri"] },
            "attribution": { "type": nullable("string") },
        },
        "required": ["result", "attribution"],
    })
}

fn distance_schema() -> Value {
    let end = json!({ "type": "object", "properties": {
        "lat": { "type": "number" }, "lon": { "type": "number" },
        "id": { "type": nullable("string") },
    }, "required": ["lat", "lon", "id"] });
    json!({
        "type": "object",
        "properties": {
            "result": { "type": "object", "properties": {
                "meters": { "type": "number" },
                "bearing": { "type": "number", "description": "Degrees clockwise from north." },
                "from": end, "to": end,
            }, "required": ["meters", "bearing", "from", "to"] },
            "attribution": { "type": nullable("string") },
        },
        "required": ["result", "attribution"],
    })
}

fn coverage_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "results": { "type": "array", "items": { "type": "object", "properties": {
                "id": { "type": "string" }, "name": { "type": nullable("string") },
                "lat": { "type": "number" }, "lon": { "type": "number" },
                "countries": { "type": "array", "items": { "type": "string" } },
                "packs": { "type": "array", "items": { "type": "object", "properties": {
                    "path": { "type": "string" }, "release": { "type": "string" },
                    "download": { "type": "integer" }, "disk": { "type": "integer" },
                    "components": { "type": "array", "items": { "type": "string" } },
                    "parts": { "type": "array", "items": { "type": "object" } },
                    "licence": { "type": "string" },
                    "state": { "type": "string", "enum": ["installed", "update", "available"] },
                }, "required": ["path", "release", "download", "disk", "components", "state"] } },
            }, "required": ["id", "name", "lat", "lon", "countries", "packs"] } },
            "attribution": { "type": nullable("string") },
        },
        "required": ["results", "attribution"],
    })
}

fn packs_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "packs": { "type": "array", "items": { "type": "object", "properties": {
                "address": { "type": "string" }, "component": { "type": "string" },
                "release": { "type": "string" }, "places": { "type": "integer" },
                "components": { "type": "array", "items": { "type": "string" } },
                "attribution": { "type": nullable("string") },
            }, "required": ["address", "component", "release", "places", "components", "attribution"] } },
        },
        "required": ["packs"],
    })
}
