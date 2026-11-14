---
title: "Servers: the MCP server and the HTTP server (phase 6)"
status: research
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
- **HTTP stack:** decided by an A/B measurement before the server is written (§5): `axum`
  0.8.9 over hyper 1.12 against bare `hyper` with a hand router. Both use tokio and serve
  rmcp's `tower` service. axum is the default if the overhead is within noise.
- **Engine calls** take microseconds: run inline on the runtime's worker, not through
  `spawn_blocking`, unless the A/B shows otherwise; the store's network work stays in its
  own jobs (D-274).
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
what we cannot fill (`osm_type` and `osm_id` exist only for OSM-derived rows; `importance`
is our score, documented as such). The conformance corpus is a set of client requests with
the fields each client reads.

**What exists to reuse.** `tgd` already has native, GeoJSON and Nominatim-shaped outputs in
`crates/teigeodata-cli/src/output.rs`, written to move to the server. The engine answers
with `Shared` snapshots that swap atomically on an update (D-141, D-172), so a server keeps
serving while packs update. Attribution per pack is in the pack's notices (D-126).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| axum 0.8 on hyper 1 | Routing and extractors, `tower-http` middleware (limits, timeouts, CORS), the most used Rust web stack, mounts rmcp's service directly | One more layer of dependencies | MIT | 0.8.9 | Default; A/B pending |
| Bare hyper 1 with a hand router | Fewest dependencies, nothing between the socket and the engine | Limits, timeouts and routing written by us | MIT | 1.12.0 | The A/B's other arm |
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
