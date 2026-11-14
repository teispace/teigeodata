---
title: "Phase 6: servers"
status: draft
updated: 2026-11-15
---

# Phase 6: servers

**Goal:** the engine and the store behind an MCP server at the latest protocol revision and an
HTTP server with native, GeoJSON and Nominatim-compatible outputs, both over one request core
whose seams (auth, limits, metering, tenants) let a hosted tier be a deployment, not a fork.
Exit: milestone **M6** ([roadmap](../roadmap.md)).

**Research:** [servers](../../02-research/notes/servers.md) (2026-11-15); the MCP part of
[bindings and MCP](../../02-research/notes/bindings-and-mcp.md) §3.13; ADR-0017, ADR-0025,
ADR-0028, ADR-0038; D-032 (attribution), D-048 (outputs).

Order: measure the stack, then the shared core, then the HTTP server (it carries MCP's HTTP
transport), then MCP, then load and safety. Nothing names a country (P16); the network stays
behind the store and an explicit call (ADR-0012); every bound is a number with its gate.

## Steps

### 6.0 Research and the A/B
- [x] The servers note: versions live, options, what is ours (2026-11-15)
- [x] The HTTP A/B: `get` and `search` on axum and on bare hyper, at 1, 16 and 64
  connections (requests per second, p50, p99, peak footprint), and inline engine calls
  against `spawn_blocking`; the stack chosen and recorded (2026-11-15, D-309: axum, engine
  calls inline; [servers §5.1](../../02-research/notes/servers.md#51-the-ab-measured-2026-11-15))

### 6.1 The request core (`teigeodata-server`)
- [x] The pack set: files or a store, served through `Shared` snapshots that follow updates
  (D-141, D-172); the journal read without opening the store (2026-11-15, D-311)
- [x] The query model with its bounds (query length, `limit` at most 50 by default, radius;
  time by the server's timeout, the inputs bounding the engine's work), the error model
  (stable codes; that no panic reaches a client is 6.2's, at the handler) (2026-11-15, D-311)
- [x] Outputs moved from the CLI (`output.rs`): native, GeoJSON, Nominatim-compatible; one
  mapping each, used by the CLI and both servers (2026-11-15, D-310)
- [x] Attribution on every answer from main data (D-032), from the notices of the packs that
  answered (2026-11-15, D-311)
- [x] The hosted seams as traits with open defaults: who is asking, may they, how much they
  used (ADR-0025) (2026-11-15, D-311)

### 6.2 The HTTP server
- [x] Routes for every engine query, `/packs` and `/health`; `tgd serve` (bound to
  127.0.0.1 by default) (2026-11-15, D-312)
- [ ] Nominatim-compatible `/search`, `/reverse`, `/lookup` (ADR-0038) with a conformance
  corpus of client requests
- [x] Limits and timeouts, CORS off unless configured, a panic answered `internal`
  (2026-11-15, D-312)
- [x] Request fuzzing of the query parsing (2026-11-15, D-313)

### 6.3 The MCP server
- [ ] stdio on rmcp: `server/discover`, the tools of ADR-0017 with `outputSchema`,
  `structuredContent` and a short text, `readOnlyHint`, bounded output with signed cursors,
  resources for datasets and places, attribution in `_meta`; `tgd mcp`
- [ ] Stateless Streamable HTTP in the HTTP server, `Origin` checked
- [ ] Downloads: install as a task (the tasks extension) where the client has it, a handle and
  a status tool otherwise; size confirmed by elicitation when the client offers it
- [ ] The MCP conformance suite in CI for 2026-07-28 and 2025-11-25 clients

### 6.4 Load, safety and size
- [ ] Load tests with the numbers gated (requests per second, p99, footprint bounded under
  load), on the Nepal pack and on the world baseline
- [ ] `tgd` size with and without the servers; the servers behind CLI features

### 6.5 Exit review
- [ ] M6 criteria met, each with its evidence; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Phase 6 drafted from the servers research | M5 met (D-308) |
| 2026-11-15 | 6.0 done: the HTTP A/B, axum with inline engine calls (D-309) | measured |
| 2026-11-15 | 6.1 done: the request core (D-310, D-311); the panic guard moves to 6.2's handler | built |
| 2026-11-15 | 6.2: native routes and `tgd serve` (D-312); fuzzing split into its own item | built |
