//! The MCP server (phase 6.3, feature `mcp`, [D-315]): the queries of [`crate::query`] as
//! Model Context Protocol tools and resources, over rmcp (ADR-0028). It speaks revision
//! 2026-07-28 (`server/discover`, no handshake) and serves 2025-11-25 clients and older ones
//! through their `initialize`.
//!
//! **Tools** ([`TOOLS`]): read-only `search`, `reverse`, `nearby`, `get_place`, `lookup`,
//! `admin_at`, `hierarchy`, `children`, `timezone`, `elevation`, `postal`, `boundary`,
//! `distance` and `packs` (D-317); over a store `coverage` ([`STORE_TOOLS`]), and on a server
//! that installs ([`Mcp::installing`]) `install` and `install_status` ([`INSTALL_TOOLS`],
//! feature `store`, D-318): an install runs as a task for a client with the tasks extension,
//! else as a handle, after the person confirms its size (MRTR elicitation, or
//! `elicitation/create` on a [duplex](Mcp::duplex) transport), else within `max_bytes`. Each has
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
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, CompleteRequestMethod,
    CompleteRequestParams, CompleteResult, ContentBlock, Implementation, JsonObject,
    ListPromptsRequestMethod, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
    ListToolsResult, MetaObject, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ResourceTemplate,
    ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{MaybeSendFuture, RequestContext};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};

use crate::body::{self, Asked, Page, WORK};
#[cfg(feature = "store")]
use crate::install::{self, Jobs};
use crate::output::{self, Bufs};
use crate::packs::Packs;
use crate::query::{self, Ask, At, Code, Limits, PostalAsk, QueryError};
use crate::seams::{Bare, Caller, Incoming, Operation, Seams, Usage};
#[cfg(feature = "store")]
use rmcp::model::{
    CancelTaskParams, ClientResult, CreateTaskResult, ElicitRequest, ElicitRequestParams,
    ElicitationAction, GetTaskParams, GetTaskResult, InputRequest, InputRequests,
    InputRequiredResult, InputResponses, ProtocolVersion, ServerRequest, UpdateTaskParams,
};
#[cfg(feature = "store")]
use rmcp::service::PeerRequestOptions;

/// The deepest place a cursor reaches: five pages of the most results [`Limits::SERVER`]
/// allows.
pub const DEPTH: usize = 250;

/// The `_meta` key of an answer's attribution.
pub const ATTRIBUTION: &str = "teigeodata/attribution";

/// The tools, in the order `tools/list` gives them.
pub const TOOLS: [&str; 17] = [
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
    "install",
    "install_status",
];

/// The tools that read the store's catalog: listed only on a server over a store (D-318).
pub const STORE_TOOLS: [&str; 1] = ["coverage"];

/// The tools that install into the store: listed only on a server over a store that allows
/// installs (D-318).
pub const INSTALL_TOOLS: [&str; 2] = ["install", "install_status"];

/// The key of the size confirmation among a call's `inputRequests`.
#[cfg(feature = "store")]
const CONFIRM: &str = "confirm";

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

/// The MCP server over a pack set: clone it freely (it is one `Arc`). The flag: the transport
/// carries requests from the server to the client ([`Mcp::duplex`]).
#[derive(Clone)]
pub struct Mcp(
    Arc<Inner>,
    #[cfg_attr(
        not(feature = "store"),
        expect(dead_code, reason = "read by `install`, built with `store`")
    )]
    bool,
);

struct Inner {
    packs: Arc<Packs>,
    seams: Seams,
    limits: Limits,
    key: [u8; 32],
    tools: Vec<Tool>,
    /// The install jobs, on a server that installs (D-318).
    #[cfg(feature = "store")]
    jobs: Option<Jobs>,
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
        Self::build(packs, seams, limits, false)
    }

    /// [`Mcp::new`], with the `install` and `install_status` tools when `packs` follow a
    /// store: an agent may install the catalog's packs into it (D-318).
    ///
    /// # Errors
    /// The random source failed.
    #[cfg(feature = "store")]
    pub fn installing(packs: Arc<Packs>, seams: Seams, limits: Limits) -> std::io::Result<Self> {
        Self::build(packs, seams, limits, true)
    }

    fn build(
        packs: Arc<Packs>,
        seams: Seams,
        limits: Limits,
        install: bool,
    ) -> std::io::Result<Self> {
        let store = packs.root().is_some();
        let install = install && store && cfg!(feature = "store");
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key).map_err(std::io::Error::other)?;
        Ok(Self(
            Arc::new(Inner {
                packs,
                seams,
                limits,
                key,
                tools: tools(&limits)
                    .into_iter()
                    .filter(|t| store || !STORE_TOOLS.contains(&t.name.as_ref()))
                    .filter(|t| install || !INSTALL_TOOLS.contains(&t.name.as_ref()))
                    .collect(),
                #[cfg(feature = "store")]
                jobs: install.then(Jobs::default),
            }),
            false,
        ))
    }

    /// This server on a transport that carries requests from the server to the client, as
    /// standard input and output do ([`serve_stdio`] sets it); stateless HTTP does not. There a
    /// 2025-11-25 client that declared elicitation confirms an install's size through
    /// `elicitation/create` (D-318).
    #[must_use]
    pub fn duplex(self) -> Self {
        Self(self.0, true)
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
        #[cfg(feature = "store")]
        match name {
            "install_status" => return self.install_status(&a, incoming),
            "install" => {
                return match self.install(&args, incoming, &Confirm::default(), start)? {
                    Step::Answer(CallToolResponse::Complete(r)) => Ok(r),
                    _ => Err(QueryError::new(Code::Internal, "the install failed")),
                };
            }
            _ => {}
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

    /// The `install` tool (D-318): what the call would download, confirmed by the person when
    /// the client can ask (MRTR elicitation in 2026-07-28, else [`Step::Ask`] for
    /// `elicitation/create`), else held to `max_bytes`; then a job, as a task for a client with
    /// the tasks extension, else a handle for `install_status`.
    #[cfg(feature = "store")]
    fn install(
        &self,
        given: &Map<String, Value>,
        incoming: &dyn Incoming,
        confirm: &Confirm<'_>,
        start: Instant,
    ) -> Result<Step, QueryError> {
        let (Some(jobs), Some(root)) = (&self.0.jobs, self.0.packs.root()) else {
            return Err(QueryError::new(Code::NotFound, "no such tool"));
        };
        let caller = self.0.seams.admit(incoming, Operation::Install)?;
        // A retry after the confirmation: the call is the one sealed in its state, whatever
        // the retry's own arguments say.
        let (args, answer) = match confirm.state {
            Some(state) => {
                let (sealed, _) = self.open_cursor("install", state)?;
                let accepted = confirm
                    .responses
                    .and_then(|r| r.get(CONFIRM))
                    .is_some_and(|v| {
                        v.get("action").and_then(Value::as_str) == Some("accept")
                            && v.get("content").and_then(|c| c.get("confirm"))
                                == Some(&Value::Bool(true))
                    });
                (sealed, Some(accepted))
            }
            None => (given.clone(), confirm.answered),
        };
        let a = Args(&args);
        let packs = a.strings("packs")?;
        let components = a.strings("components")?;
        let max_bytes = a.whole("max_bytes")?.unwrap_or(install::DEFAULT_MAX_BYTES);
        let plan = install::plan(root, &packs, &components)?;
        let size = install::megabytes(plan.download);
        match answer {
            Some(false) => {
                let declined = json!({ "status": "declined", "packs": plan.paths,
                                       "download": plan.download });
                return Ok(Step::Answer(CallToolResponse::Complete(done(
                    json!({ "result": declined, "attribution": null }),
                    None,
                ))));
            }
            None if plan.download > 0 && confirm.elicit => {
                let question = format!("Download {size} to install {}?", plan.paths.join(", "));
                let mut requests = InputRequests::new();
                requests.insert(CONFIRM.into(), confirmation(&question)?);
                let state = self.cursor("install", &args, 0);
                return Ok(Step::Answer(CallToolResponse::InputRequired(
                    InputRequiredResult::new(Some(requests), Some(state)),
                )));
            }
            None if plan.download > 0 && confirm.ask => {
                let question = format!("Download {size} to install {}?", plan.paths.join(", "));
                return Ok(Step::Ask(question));
            }
            None if plan.download > max_bytes => {
                return Err(QueryError::new(
                    Code::OutOfBounds,
                    format!(
                        "the download is {size} ({} bytes), past `max_bytes` ({max_bytes}): \
                         call again with `max_bytes` at least {} to install it",
                        plan.download, plan.download
                    ),
                ));
            }
            _ => {}
        }
        let mut raw = [0_u8; 16];
        getrandom::fill(&mut raw)
            .map_err(|_| QueryError::new(Code::Internal, "the install could not start"))?;
        let id: String = raw
            .iter()
            .flat_map(|b| [hex(b >> 4), hex(b & 15)])
            .collect();
        let job = jobs.start(
            id,
            root.to_path_buf(),
            plan.clone(),
            Arc::clone(&self.0.packs),
        )?;
        let response = if confirm.tasks {
            CallToolResponse::Task(CreateTaskResult::new(job.task().task))
        } else {
            let mut status = job.status();
            if let Some(o) = status.as_object_mut() {
                o.insert("packs".into(), json!(plan.paths));
                o.insert("download".into(), json!(plan.download));
            }
            CallToolResponse::Complete(done(json!({ "result": status, "attribution": null }), None))
        };
        self.0.seams.meter.record(
            &caller,
            Operation::Install,
            Usage {
                results: plan.paths.len(),
                bytes: usize::try_from(plan.download).unwrap_or(usize::MAX),
                micros: u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
            },
        );
        Ok(Step::Answer(response))
    }

    /// A `tools/call` of `install`: [`Mcp::install`], asking the person first through
    /// `elicitation/create` when it says so, then again with their answer.
    #[cfg(feature = "store")]
    async fn install_call(
        &self,
        request: &CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResponse {
        let empty = JsonObject::new();
        let args = request.arguments.as_ref().unwrap_or(&empty);
        let start = Instant::now();
        let run = |confirm: &Confirm<'_>| {
            incoming(context, |i| {
                catch_unwind(AssertUnwindSafe(|| self.install(args, i, confirm, start)))
                    .unwrap_or_else(|_| Err(QueryError::new(Code::Internal, "the server failed")))
            })
        };
        let mut confirm = Confirm::of(request, context, self.1);
        let mut step = run(&confirm);
        if let Ok(Step::Ask(question)) = &step {
            step = match ask(context, question).await {
                Ok(accepted) => {
                    confirm.answered = Some(accepted);
                    run(&confirm)
                }
                Err(e) => Err(e),
            };
        }
        match step {
            Ok(Step::Answer(response)) => response,
            Ok(Step::Ask(_)) => CallToolResponse::Complete(failed(&QueryError::new(
                Code::Internal,
                "the server failed",
            ))),
            Err(e) => CallToolResponse::Complete(failed(&e)),
        }
    }

    /// The `install_status` tool: where the install of a handle is.
    #[cfg(feature = "store")]
    fn install_status(
        &self,
        a: &Args<'_>,
        incoming: &dyn Incoming,
    ) -> Result<CallToolResult, QueryError> {
        let jobs = self
            .0
            .jobs
            .as_ref()
            .ok_or_else(|| QueryError::new(Code::NotFound, "no such tool"))?;
        self.0.seams.admit(incoming, Operation::Install)?;
        let handle = a.need_text("handle")?;
        let job = jobs.get(&handle).ok_or_else(|| {
            QueryError::new(
                Code::NotFound,
                format!("no install {handle:?}: it ended over an hour ago, or never began"),
            )
        })?;
        Ok(done(
            json!({ "result": job.status(), "attribution": null }),
            None,
        ))
    }

    /// The install job `id` as a task, for `tasks/*`.
    #[cfg(feature = "store")]
    fn job(&self, id: &str) -> Result<Arc<install::Job>, ErrorData> {
        self.0
            .jobs
            .as_ref()
            .and_then(|j| j.get(id))
            .ok_or_else(|| ErrorData::invalid_params(format!("no task {id:?}"), None))
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

    /// `resources/read` of `uri` without a transport, for the fuzz harness.
    #[cfg(feature = "fuzz")]
    pub(crate) fn read_for_fuzz(
        &self,
        uri: &str,
        incoming: &dyn Incoming,
    ) -> Result<ResourceContents, QueryError> {
        self.read(uri, incoming)
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
        let r = output::full(&e, &item, &mut bufs, false)?;
        let mut f = Vec::with_capacity(1024);
        output::feature_full(&mut f, &item.place, r)
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
    let running = match server.duplex().serve(rmcp::transport::stdio()).await {
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
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources();
        // Installs run as tasks for a client that declares the extension (D-318).
        #[cfg(feature = "store")]
        let capabilities = if self.0.jobs.is_some() {
            capabilities.enable_tasks()
        } else {
            capabilities
        };
        ServerConfig::new(capabilities.build())
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

    #[cfg_attr(
        not(feature = "store"),
        expect(
            clippy::unused_async_trait_impl,
            reason = "only `install` (feature `store`) awaits"
        )
    )]
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        #[cfg(feature = "store")]
        if request.name == "install" && self.0.jobs.is_some() {
            return Ok(self.install_call(&request, &context).await);
        }
        let empty = JsonObject::new();
        let args = request.arguments.as_ref().unwrap_or(&empty);
        incoming(&context, |i| self.call(&request.name, args, i))
            .map(CallToolResponse::from)
            .ok_or_else(|| ErrorData::invalid_params(format!("no tool {:?}", request.name), None))
    }

    fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(incoming(&context, |i| self.datasets(i)).map_err(protocol))
    }

    // Neither prompts nor completions are declared: their methods are not found, rather
    // than rmcp's default empty answers, which would claim a capability the server lacks.
    fn list_prompts(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Err(
            ErrorData::method_not_found::<ListPromptsRequestMethod>(),
        ))
    }

    fn complete(
        &self,
        _: CompleteRequestParams,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CompleteResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Err(ErrorData::method_not_found::<CompleteRequestMethod>()))
    }

    fn list_resource_templates(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + MaybeSendFuture + '_
    {
        let items = resource_templates();
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(items)
            .with_ttl_ms(TOOLS_TTL_MS)
            .with_cache_scope(CacheScope::Public)))
    }

    #[cfg(feature = "store")]
    fn get_task(
        &self,
        request: GetTaskParams,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetTaskResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(
            self.job(&request.task_id)
                .map(|j| GetTaskResult::new(j.task())),
        )
    }

    #[cfg(feature = "store")]
    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(self.job(&request.task_id).map(|_| {
            if let Some(jobs) = &self.0.jobs {
                jobs.cancel(&request.task_id);
            }
        }))
    }

    #[cfg(feature = "store")]
    fn update_task(
        &self,
        _: UpdateTaskParams,
        _: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), ErrorData>> + MaybeSendFuture + '_ {
        // The size is confirmed before an install starts: a running one asks nothing.
        std::future::ready(Err(ErrorData::invalid_params(
            "an install asks for no input while it runs",
            None,
        )))
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
        std::future::ready(read.map_err(|e| unread(e, &request.uri)).map(|contents| {
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
    port: Option<u16>,
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
    // With no origins, every `Origin` sent is refused (a native client sends none). On
    // loopback the server's own origin is accepted: same-origin, never a rebinding.
    let own = port
        .filter(|_| !hosts.is_empty())
        .into_iter()
        .flat_map(|p| {
            hosts.iter().map(move |h| {
                if h.contains(':') {
                    format!("http://[{h}]:{p}")
                } else {
                    format!("http://{h}:{p}")
                }
            })
        });
    let origins: Vec<String> = origins
        .iter()
        .filter_map(|o| o.to_str().ok())
        .map(with_port)
        .chain(own)
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

/// What the server tells a client about itself (`instructions`).
pub const INSTRUCTIONS: &str = "Offline place data. Find places by name with `search`, by \
coordinates with `reverse` and `admin_at`, around a place with `nearby`; climb with \
`hierarchy` and descend with `children`. Places are named by IDs such as tgd:np/1xhb: pass \
them to `get_place`, `lookup`, `nearby`, `timezone` or read teigeodata://place/{id}. \
Coordinates are WGS84 degrees, distances metres, heights metres above EGM2008. A long answer \
has a `next_cursor`: call the same tool with only `cursor` for the next page. Show the \
`attribution` of an answer wherever its data is shown: the licences require it.";

/// A tool error: the stable code and the message, as the HTTP server answers them.
pub(crate) fn failed(e: &QueryError) -> CallToolResult {
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

/// A failed `resources/read` of `uri`: [`protocol`]'s error, carrying the URI when it is not
/// found (SEP-2164: `data.uri`, so a client knows which of its reads failed).
fn unread(e: QueryError, uri: &str) -> ErrorData {
    let missing = e.code == Code::NotFound;
    let mut error = protocol(e);
    if missing && let Some(Value::Object(data)) = error.data.as_mut() {
        data.insert("uri".into(), uri.into());
    }
    error
}

/// A finished answer: `v` as structured content and as text, `attribution` in `_meta`.
pub(crate) fn done(v: Value, attribution: Option<String>) -> CallToolResult {
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

    /// A list of strings; empty when not given.
    #[cfg(feature = "store")]
    fn strings(&self, name: &str) -> Result<Vec<String>, QueryError> {
        match self.get(name) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or_else(|| bad(&format!("`{name}` must be strings"))),
            Some(_) => Err(bad(&format!("`{name}` must be an array of strings"))),
        }
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

/// Where an install call stands: answered, or the person must first be asked `question`
/// through `elicitation/create` (a 2025-11-25 client on a duplex transport).
#[cfg(feature = "store")]
enum Step {
    Answer(CallToolResponse),
    Ask(String),
}

/// How long the person has to answer an `elicitation/create` confirmation.
#[cfg(feature = "store")]
const ASK_TIMEOUT: Duration = Duration::from_secs(600);

/// What a client can do for an install call: run it as a task, confirm its size by MRTR
/// elicitation (`elicit`) or by `elicitation/create` (`ask`), the answer and sealed state of an
/// MRTR retry, and the answer to `elicitation/create` once asked.
#[cfg(feature = "store")]
#[derive(Default)]
struct Confirm<'a> {
    tasks: bool,
    elicit: bool,
    ask: bool,
    responses: Option<&'a InputResponses>,
    state: Option<&'a str>,
    answered: Option<bool>,
}

#[cfg(feature = "store")]
impl<'a> Confirm<'a> {
    fn of(
        request: &'a CallToolRequestParams,
        context: &RequestContext<RoleServer>,
        duplex: bool,
    ) -> Self {
        let caps = context.client_capabilities();
        // MRTR is 2026-07-28's; an older client is asked over the session where the transport
        // carries it, else gets the `max_bytes` bound.
        let current = context
            .protocol_version()
            .is_some_and(|v| v >= ProtocolVersion::V_2026_07_28);
        // An empty `elicitation` declares form mode (2025-11-25 compatibility).
        let form = caps
            .as_ref()
            .and_then(|c| c.elicitation.as_ref())
            .is_some_and(|e| e.form.is_some() || e.url.is_none());
        Self {
            tasks: caps
                .as_ref()
                .is_some_and(rmcp::model::ClientCapabilities::supports_tasks),
            elicit: current && form,
            ask: !current && duplex && form,
            responses: request.input_responses.as_ref(),
            state: request.request_state.as_deref(),
            answered: None,
        }
    }
}

/// Asks the person through `elicitation/create` (2025-11-25) to confirm `question`: whether
/// they accepted with `confirm` true.
///
/// # Errors
/// [`Code::Unavailable`] when the client did not answer within [`ASK_TIMEOUT`].
#[cfg(feature = "store")]
async fn ask(context: &RequestContext<RoleServer>, question: &str) -> Result<bool, QueryError> {
    let InputRequest::Elicitation(request) = confirmation(question)? else {
        return Err(QueryError::new(Code::Internal, "the confirmation failed"));
    };
    let unanswered = || {
        QueryError::new(
            Code::Unavailable,
            "the confirmation was not answered: nothing was installed",
        )
    };
    // The peer's generic request: rmcp's `create_elicitation` needs its `elicitation` feature,
    // which brings `url` and the ICU tables for one call.
    let mut options = PeerRequestOptions::no_options();
    options.timeout = Some(ASK_TIMEOUT);
    let answer = context
        .peer
        .send_request_with_option(ServerRequest::ElicitRequest(request), options)
        .await
        .map_err(|_| unanswered())?
        .await_response()
        .await
        .map_err(|_| unanswered())?;
    let ClientResult::ElicitResult(answer) = answer else {
        return Err(unanswered());
    };
    Ok(answer.action == ElicitationAction::Accept
        && answer.content.as_ref().and_then(|c| c.get("confirm")) == Some(&Value::Bool(true)))
}

/// The form asking the person to confirm a download.
#[cfg(feature = "store")]
fn confirmation(question: &str) -> Result<InputRequest, QueryError> {
    let schema = serde_json::from_value(json!({
        "type": "object",
        "properties": { "confirm": { "type": "boolean", "title": "Download", "default": true } },
        "required": ["confirm"],
    }))
    .map_err(|_| QueryError::new(Code::Internal, "the confirmation failed"))?;
    Ok(InputRequest::Elicitation(ElicitRequest::new(
        ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: question.to_owned(),
            requested_schema: schema,
        },
    )))
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

/// The resource templates, as `resources/templates/list` gives them.
#[must_use]
pub fn resource_templates() -> Vec<ResourceTemplate> {
    vec![
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
    ]
}

/// Every tool's definition, the store's and the installer's included, in [`TOOLS`] order: what
/// `tools/list` gives on a server that has them all, and what the reference page is generated
/// from.
#[must_use]
pub fn tool_definitions(limits: &Limits) -> Vec<Tool> {
    tools(limits)
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
            "Places matching a name, best first. \"Name, Parent\" ranks places under a parent of that name first. `focus` ranks places near a point higher (scale_km: how far counts as near, default 50). `strict` ranks by how well the name matched (exact, then loose, prefix, one edit) before importance and nearness.",
            json!({
                "query": { "type": "string", "minLength": 1, "maxLength": limits.text_bytes, "description": "The name, in any script, such as Pokhara or पोखरा." },
                "limit": limit, "cursor": cursor, "strict": { "type": "boolean", "default": false, "description": "Rank by match quality first." },
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
        def(
            "install",
            "Install packs",
            "Downloads packs from the catalog into the store, verified, and serves them: `packs` as catalog paths or addresses (np, np.places, ae-du), `components` (every one when none). The person confirms the download's size when the client can ask; otherwise the call is refused past `max_bytes` (default 100 MB), naming the size. Answers a task, or a `handle` for `install_status`.",
            json!({
                "packs": { "type": "array", "items": { "type": "string", "minLength": 1, "maxLength": limits.text_bytes }, "minItems": 1, "maxItems": install_limit() },
                "components": { "type": "array", "items": { "type": "string", "enum": component_names() } },
                "max_bytes": { "type": "integer", "minimum": 0, "description": "The most this call may download without the person's confirmation; default 100,000,000." },
            }),
            &["packs"],
            &install_schema(),
        )
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        ),
        def(
            "install_status",
            "How an install goes",
            "Where the install of a `handle` (from `install`) is: working, completed, failed or cancelled, with its message and, once ended, what it installed or why it failed. Ask about once a second.",
            json!({ "handle": { "type": "string", "minLength": 32, "maxLength": 32 } }),
            &["handle"],
            &install_schema(),
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
                    "id": { "type": "string", "description": "Its stable ID, such as tgd:np/1xhb." },
                    "name": { "type": nullable("string"), "description": "Its canonical name." },
                    "kind": { "type": nullable("string"), "description": "Its kind, such as settlement or admin." },
                    "admin_level": { "type": "integer", "description": "1 for a country's first-level units, deeper units after; 0 for a country and for a place that is not an admin unit." },
                    "lat": { "type": "number", "description": "Latitude, WGS84 degrees." },
                    "lon": { "type": "number", "description": "Longitude, WGS84 degrees." },
                    "elevation": { "type": nullable("integer"), "description": "Metres above EGM2008." },
                    "elevation_source": { "type": "string", "description": "Where the height came from: surveyed or model, when stated." },
                    "parent": { "type": nullable("string"), "description": "Its parent's ID; null for a root." },
                    "timezone": { "type": nullable("string"), "description": "Its IANA timezone, such as Asia/Kathmandu." },
                    "iso_code": { "type": "string", "description": "Its ISO 3166 code, for a country or unit that has one." },
                    "population": { "type": "integer", "description": "Its number of people, when known." },
                    "population_kind": { "type": "string", "description": "Where the number came from: stated (a source's count) or modelled (a population grid over an admin unit)." },
                    "class": { "type": "string", "description": "Its class, when it has one: a landmark's (temple, palace, monastery, shrine, religious_site, monument, museum, historic_site, castle, ruin, pagoda, mosque, church, airport)." },
                    "meters": { "type": "number", "description": "Distance from the point or place asked about, metres." },
                    "bearing": { "type": "number", "description": "Degrees clockwise from north, from the place to the point asked about." },
                    "inside": { "type": "boolean", "description": "Whether the point asked about is inside the place's outline (reverse)." },
                    "score": { "type": "integer", "description": "Its search score, comparable only within one query." },
                    "names": { "type": "array", "description": "Every name, canonical first (get_place with names).", "items": { "type": "object", "properties": {
                        "text": { "type": "string" },
                        "lang": { "type": "string", "description": "A BCP 47 tag, such as ne-Deva." },
                        "kind": { "type": "string", "description": "The name's type." },
                        "origin": { "type": "string", "description": "Where the name came from." },
                        "machine": { "type": "boolean", "description": "Made by a machine (a romanisation, a transliteration or a donor label): an app may show it differently." } },
                        "required": ["text", "origin", "machine"] } },
                    "utc_offset": { "type": "string", "description": "The UTC offset at the instant asked, such as +05:45 (timezone)." },
                    "tz_abbr": { "type": "string", "description": "The zone's abbreviation at that instant." },
                    "is_dst": { "type": "boolean", "description": "Whether daylight saving time is in force at that instant." },
                },
                "required": ["id", "name", "kind", "admin_level", "elevation", "parent", "timezone"],
            } },
            "attribution": { "type": nullable("string"), "description": "The credits of the packs that answered: show them wherever the data is shown." },
            "next_cursor": { "type": "string", "description": "Pass as `cursor` to the same tool for the next page; absent on the last." },
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

/// The most packs one install names.
const fn install_limit() -> usize {
    16
}

/// Every component's name, for the `install` tool's schema.
fn component_names() -> Vec<&'static str> {
    teigeodata_engine::Components::all()
        .iter()
        .map(teigeodata_engine::Component::name)
        .collect()
}

fn install_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "result": { "type": "object", "properties": {
                "handle": { "type": "string" },
                "status": { "type": "string", "enum": ["working", "completed", "failed", "cancelled", "declined"] },
                "message": { "type": "string" },
                "packs": { "type": "array", "items": { "type": "string" } },
                "download": { "type": "integer" },
                "result": { "type": ["object", "null"] },
                "error": { "type": ["object", "null"] },
            }, "required": ["status"] },
            "attribution": { "type": "null" },
        },
        "required": ["result", "attribution"],
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
