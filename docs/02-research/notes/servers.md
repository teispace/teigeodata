---
title: "Servers: the MCP server and the HTTP server (phase 6)"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-02-15
---

# Servers: the MCP server and the HTTP server

## 1. Question

How phase 6 builds M6, "MCP server (latest revision) and HTTP server, hosted-ready seams",
on what is decided already (ADR-0017 MCP from day 0, ADR-0028 the MCP baseline, ADR-0025
hosted-ready, ADR-0038 compatibility outputs, D-032 attribution), with versions checked now,
and which choices still need a measurement before code.

## 2. Summary and recommendation

- **One request core, two front ends.** A crate `teigeodata-server` holds what both servers
  share: the pack set (files or the store, served through `Shared` snapshots, D-141), the
  query model with its limits, the outputs (the CLI's `output.rs` moves here, as it says:
  native JSON, GeoJSON, Nominatim-compatible), attribution on every answer (D-032), and the
  hosted-ready seams as traits with open defaults (ADR-0025). The HTTP server and the MCP
  server are thin front ends over it; `tgd serve` and `tgd mcp` run them.
- **MCP:** revision **2026-07-28** stays the target, with 2025-11-25 clients served (no newer
  revision exists, checked 2026-11-15), on **`rmcp` 3.10.0**. stdio first, then stateless
  Streamable HTTP mounted in the same HTTP server (rmcp's server transport is a `tower`
  service). The tool, resource and output rules of the bindings note §3.13 stand.
- **HTTP stack: axum** 0.8.9 over hyper 1.12 (measured, §5.1: within the noise of bare
  hyper, with routing and `tower-http` limits built in). It serves rmcp's `tower` service.
- **Engine calls run inline** on the runtime's worker (measured, §5.1: 1.4 to 1.9 times the
  throughput of `spawn_blocking`, a far shorter p99, 60% of the footprint); the store's
  network work stays in its own jobs (D-274).
- **Secure by default:** bind 127.0.0.1, `Origin` checked (MCP requires it), CORS off unless
  configured, request size, query length, `limit` and time bounded, no panic reaches a
  client (a 500 with an error code).

## 3. Findings

**MCP revision (checked 2026-11-15).** The specification's changelog has no revision after
2026-07-28; its changes from 2025-11-25 are in the bindings note §3.13 (stateless lifecycle,
`server/discover`, sessionless Streamable HTTP, MRTR, tasks as an extension, required
`resultType`, `ttlMs` and `cacheScope` on list and read results).

**rmcp 3.10.0 (2026-11-12).** Since 3.9.0: loading Skills from a directory tree, a fix
keeping tools whose schemas have dependency maps, and documentation of the `Send` limit on
async `#[tool]` bodies and of the stdio lifecycle. Features we would use: `server`
(default), `macros`, `transport-io` (stdio), `transport-streamable-http-server` (a
`tower-service` over `http` and `http-body`, no web framework), `elicitation`. Its own tests
serve the HTTP transport with axum 0.8 and with bare hyper 1. The release cadence is about
weekly (3.8.0 on 2026-10-26, 3.9.0 on 2026-11-05, 3.10.0 on 2026-11-12): pin it exactly
and review each update.

**HTTP crates (crates.io, 2026-11-15):** axum 0.8.9 (2026-04-14), hyper 1.12.0
(2026-10-06), hyper-util 0.1.21, tower-http 0.7.1 (timeouts, body limits, CORS,
compression), tokio 1.54.1, actix-web 4.19.0 (its own runtime model and service trait;
rmcp's transport is a `tower` service, so actix needs an adapter: not considered further).

**Nominatim compatibility (ADR-0038).** Nominatim 5.3.3 (2026-10-14) is the current release.
Its `/search`, `/reverse` and `/lookup` with `format=json|jsonv2|geojson|geocodejson` are
what clients call. The compatible mode maps our native rows to its fields and leaves out
what we cannot fill (`osm_type` and `osm_id` exist only for OSM-derived rows). The
conformance corpus is a set of client requests with the fields each client reads. Refined by
[Nominatim compatibility](nominatim-compat.md) (2026-11-15): `importance` is left out too, as
our score is not on its scale.

**What exists to reuse.** `tgd` already has native, GeoJSON and Nominatim-shaped outputs in
`crates/teigeodata-cli/src/output.rs`, written to move to the server. The engine answers
with `Shared` snapshots that swap atomically on an update (D-141, D-172), so a server keeps
serving while packs update. Attribution per pack is in the pack's notices (D-126).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| axum 0.8 on hyper 1 | Routing and extractors, `tower-http` middleware (limits, timeouts, CORS), the most used Rust web stack, mounts rmcp's service directly | One more layer of dependencies | MIT | 0.8.9 | **Chosen** (within noise of hyper, §5.1) |
| Bare hyper 1 with a hand router | Fewest dependencies, nothing between the socket and the engine | Limits, timeouts and routing written by us | MIT | 1.12.0 | Measured equal; rejected for the code it needs |
| actix-web 4 | Fast, mature | Not `tower`: rmcp's transport needs an adapter; a second runtime model | MIT/Apache-2.0 | 4.19.0 | Rejected |
| rmcp for MCP | The official SDK, tracks 2026-07-28, conformance work | Weekly releases, API churn | Apache-2.0 | 3.10.0 | Chosen (ADR-0028) |
| Our own MCP layer | No churn | Re-implements a moving protocol | n/a | n/a | Rejected |

## 5. What we build ourselves, and why

- **The A/B (step 6.0):** the same `get` and `search` handlers on axum and on bare hyper,
  measured with a load generator on this machine at 1, 16 and 64 connections: requests per
  second, p50 and p99 latency, peak memory; and inline engine calls against
  `spawn_blocking`. The difference decides the stack; the numbers go in this note.
- **The request core:** limits, output modes, attribution and the hosted seams are ours:
  they are the product's rules, not a framework's.
- **Signed cursors:** a stateless cursor is the query and its offset, signed with a key the
  server makes at start (an HMAC; rmcp's `request-state` feature does the same for its own
  state), so paging keeps the protocol stateless.

### 5.1 The A/B, measured (2026-11-15)

`crates/teigeodata-bench/examples/http-ab.rs` on the Nepal lite pack (87 MB, mapped), `oha`
1.17.0 on the same machine (Apple M-series, 10 cores, shared: load average 12 to 18 during
the runs), 8 s per point after a 2 s warm-up, two rounds interleaved. Search is
`q=Pokhara&limit=10`; place is one `get`. Requests per second, latency p50 / p99:

| Arm | search c=1 | search c=16 | search c=64 | place c=16 | footprint |
|---|---|---|---|---|---|
| axum, inline (round 2) | 13,827, 68 / 136 µs | 86,955, 170 / 494 µs | 103,941, 599 / 1,294 µs | 116,884, 130 / 253 µs | 6.3 MB |
| hyper, inline (round 2) | 13,967, 65 / 166 µs | 93,082, 163 / 365 µs | 94,434, 638 / 1,599 µs | 116,965, 128 / 294 µs | 5.9 MB |
| axum, `spawn_blocking` (round 2) | 12,330, 79 / 108 µs | 63,369, 230 / 637 µs | 69,146, 658 / 5,475 µs | 63,191, 218 / 760 µs | 9.8 MB |
| hyper, `spawn_blocking` (round 2) | 11,742, 79 / 194 µs | 53,724, 254 / 1,106 µs | 66,751, 571 / 7,731 µs | 59,332, 219 / 1,092 µs | 12 MB |

Round 1 agreed except for axum inline at c=1 (9,108 req/s, p99 945 µs: a load spike from
another project's build). Round 1 as printed by the harness's script:

```text
r1 axum inline search c=1 rps=9108 p50=71.0us p99=945.4us ok=1.0000
r1 axum inline search c=16 rps=74729 p50=179.5us p99=673.8us ok=1.0000
r1 axum inline search c=64 rps=77174 p50=738.5us p99=2649.5us ok=1.0000
r1 axum inline place c=16 rps=115235 p50=130.5us p99=289.2us ok=1.0000
r1 axum inline footprint: 6288 KB
r1 hyper inline search c=1 rps=15083 p50=64.2us p99=98.5us ok=1.0000
r1 hyper inline search c=16 rps=89424 p50=168.1us p99=411.2us ok=1.0000
r1 hyper inline search c=64 rps=105953 p50=587.7us p99=1264.4us ok=1.0000
r1 hyper inline place c=16 rps=118481 p50=129.9us p99=233.0us ok=1.0000
r1 hyper inline footprint: 5888 KB
r1 axum blocking search c=1 rps=12312 p50=78.1us p99=173.8us ok=1.0000
r1 axum blocking search c=16 rps=61945 p50=232.4us p99=689.6us ok=1.0000
r1 axum blocking search c=64 rps=75211 p50=660.4us p99=4642.9us ok=1.0000
r1 axum blocking place c=16 rps=63673 p50=220.9us p99=686.2us ok=1.0000
r1 axum blocking footprint: 9937 KB
r1 hyper blocking search c=1 rps=12597 p50=77.2us p99=106.0us ok=1.0000
r1 hyper blocking search c=16 rps=63204 p50=229.7us p99=659.0us ok=1.0000
r1 hyper blocking search c=64 rps=77880 p50=594.6us p99=5299.5us ok=1.0000
r1 hyper blocking place c=16 rps=62811 p50=216.1us p99=799.8us ok=1.0000
r1 hyper blocking footprint: 9457 KB
```

- **Inline engine calls win:** 1.4 to 1.9 times the requests of `spawn_blocking`, a p99 at
  64 connections a third to a fifth as long, and about 60% of the footprint. Engine queries
  take microseconds; a hop to the blocking pool costs more than the query.
- **axum and bare hyper are within the noise:** axum 7% behind at c=16 and 10% ahead at
  c=64 in round 2, equal on `get`; 0.4 MB more footprint. axum is chosen: its routing,
  extractors and `tower-http` limits are what we would otherwise write.

### 5.2 The MCP server as built (2026-11-15)

What building it on rmcp 3.10.0 taught (D-315):

- **Handwritten `ServerHandler`.** The tools are the request core's asks, so rmcp's
  `#[tool]` macros (one Rust function per tool, schemas from types) would duplicate them;
  without the `macros` feature the server is a table of definitions and one dispatch. rmcp
  routes `server/discover` and the `initialize` handshake itself: a discover alone does not
  start a session, so a client that leaves after it ends `serve` with `ConnectionClosed`,
  which `tgd mcp` treats as a normal end.
- **Tool names.** MCP allows dots, but hosts that pass tools on to a model accept
  `[A-Za-z0-9_-]` only, so `snake_case` names, and the host's own prefix namespaces them.
- **Text and structure.** The 2025-11-25 specification recommends the serialized JSON as text
  beside `structuredContent` for clients that read only text; a summary would hide data from
  them. The answers are bounded (50 places a page), so the cost of the copy is bounded too.
- **Cursors.** Hex of `{tool, arguments, offset}` and its BLAKE3 keyed hash (the workspace
  already has BLAKE3; rmcp's `request-state` would add HMAC-SHA256 and base64 for the same
  property). A page is the engine's answer to `offset + limit + 1`, so the last element says
  whether a next page exists; pages stop at 250 places.
- **Standard output.** `tgd` locked standard output for the whole run; rmcp writes it from the
  runtime's threads, which waited on the lock forever. `tgd mcp` does not take it.
- **Measured on the Nepal lite pack** (release build, this machine, shared): `server/discover`
  answers in 3.8 ms from start; `tools/list` is 21.7 KB; the first `search` (pages not yet
  mapped in) 20 ms. `tgd` with both servers is 9.4 MB (phase 6.4 measures it without them).

## 6. Impact on the plan

- A phase 6 file ([phase-6](../../07-planning/phases/phase-6.md)) with the steps.
- `teigeodata-server` and the CLI's `serve` and `mcp` commands (the server crate is optional
  in the CLI behind features, so `tgd` stays small for those who only query).
- `docs/06-engineering/versions.md` gains rmcp, axum, hyper, tower-http and tokio when the
  first code lands.

## 7. Risks and open items

- rmcp's weekly releases: pinned exactly; an update is reviewed against its changelog.
- MCP clients differ in the size of tool output they accept (**unverified per client**):
  responses stay bounded (default `limit` 10, at most 50) with cursors and a `fields` selector.
- The MCP conformance suite: run in CI against both revisions; how it is installed is checked
  when the server first runs.
- A hosted tier needs auth (the 2026-07-28 authorization spec): only the seam is built now
  (D-016).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| MCP specification | 2026-07-28 (latest) | 2026-11-15 | modelcontextprotocol.io changelog |
| rmcp | 3.10.0 (2026-11-12) | 2026-11-15 | crates.io, GitHub releases |
| axum | 0.8.9 | 2026-11-15 | crates.io |
| hyper | 1.12.0 | 2026-11-15 | crates.io |
| hyper-util | 0.1.21 | 2026-11-15 | crates.io |
| tower-http | 0.7.1 | 2026-11-15 | crates.io |
| tokio | 1.54.1 | 2026-11-15 | crates.io |
| schemars | 1.2.3 | 2026-11-15 | crates.io |
| actix-web | 4.19.0 | 2026-11-15 | crates.io |
| oha (load generator) | 1.17.0 | 2026-11-15 | crates.io |
| Nominatim | 5.3.3 (2026-10-14) | 2026-11-15 | GitHub releases, PyPI `nominatim-api` |

## 9. Sources

- [MCP specification changelog (latest)](https://modelcontextprotocol.io/specification/latest/changelog),
  accessed 2026-11-15.
- [MCP 2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog),
  accessed 2026-11-15.
- [modelcontextprotocol/rust-sdk releases](https://github.com/modelcontextprotocol/rust-sdk/releases)
  and `crates/rmcp/Cargo.toml` at `rmcp-v3.10.0`, accessed 2026-11-15.
- [osm-search/Nominatim releases](https://github.com/osm-search/Nominatim/releases),
  accessed 2026-11-15.
- crates.io API for each crate above, accessed 2026-11-15.
- [Bindings, packaging, CI and MCP](bindings-and-mcp.md) §3.13 (2026-11-06), ADR-0017,
  ADR-0025, ADR-0028, ADR-0038.
