---
title: "Phase 6: servers"
status: accepted
updated: 2026-11-15
---

# Phase 6: servers

**Goal:** the engine and the store behind an MCP server at the latest protocol revision and an
HTTP server with native, GeoJSON and Nominatim-compatible outputs, both over one request core
whose seams (auth, limits, metering, tenants) let a hosted tier be a deployment, not a fork.
Exit: milestone **M6** ([roadmap](../roadmap.md)).

**Research:** [servers](../../02-research/notes/servers.md) (2026-11-15),
[Nominatim compatibility](../../02-research/notes/nominatim-compat.md) (2026-11-15); the MCP part of
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
- [x] Nominatim-compatible `/search`, `/reverse`, `/lookup` (ADR-0038) with a conformance
  corpus of client requests (2026-11-15, D-314,
  [research](../../02-research/notes/nominatim-compat.md))
- [x] Limits and timeouts, CORS off unless configured, a panic answered `internal`
  (2026-11-15, D-312)
- [x] Request fuzzing of the query parsing (2026-11-15, D-313)

### 6.3 The MCP server
- [x] stdio on rmcp: `server/discover`, the tools of ADR-0017 with `outputSchema`,
  `structuredContent` and the same JSON as text, `readOnlyHint`, bounded output with signed
  cursors, resources for datasets and places, attribution in `_meta`; `tgd mcp` (2026-11-15,
  D-315)
- [x] `boundary` (an outline, simplified to a vertex bound, as GeoJSON; in MCP a summary and a
  resource link) and `distance` (between places or points), over the request core so HTTP
  and MCP answer them alike (2026-11-15, D-317)
- [x] Stateless Streamable HTTP in the HTTP server, `Origin` checked (2026-11-15, D-316)
- [x] `coverage` (which packs hold a point, from the stored catalog), one function for
  `tgd locate`, `/v1/coverage` and the MCP tool ([research](../../02-research/notes/mcp-downloads.md), D-318; 2026-11-15)
- [x] Downloads: `install` as a task (the tasks extension) where the client has it, a handle
  and `install_status` otherwise, from one job registry; size confirmed by MRTR elicitation
  (2026-07-28) or `elicitation/create` (2025-11-25 on stdio), `max_bytes` otherwise; on for
  `tgd mcp` (`--no-install` leaves it out), opt-in for `tgd serve` (`--allow-install`)
  (D-318; 2026-11-15)
- [x] The MCP conformance suite in CI for 2026-07-28 and 2025-11-25 clients, check by check
  against a baseline of the scenarios that need the SDK fixture ([research](../../02-research/notes/mcp-conformance.md), D-319; 2026-11-15)

### 6.4 Load, safety and size
- [x] Load tests with the numbers gated (requests per second, p99, footprint bounded under
  load), on the Nepal pack and on the world baseline: `cargo xtask load --check
  tools/load/budget.toml`, proved on a planted sleep and a planted leak; boundary made 3 to 4
  times faster on the way ([research](../../02-research/notes/server-load.md), D-320; 2026-11-15)
- [x] `tgd` size with and without the servers; the servers behind CLI features: 4.59 MB
  without, 9.38 MB with both, `cargo xtask sizes --tgd`; on by default (D-320; 2026-11-15)

### 6.5 Exit review
- [x] M6 criteria met (2026-11-15, D-321), each against the [roadmap](../roadmap.md):
  - **MCP server at the latest revision:** `tgd mcp` on stdio and `/mcp` over stateless
    Streamable HTTP speak 2026-07-28 and 2025-11-25 (D-315, D-316); 17 tools of ADR-0017 over
    the request core, with output schemas, bounded output and signed cursors, resources,
    attribution in `_meta`, coverage and downloads as a task or a handle (D-317, D-318); the
    official conformance suite in CI for both revisions, check by check (D-319),
    `mcp-conformance` green at `b8c06dd` (run 43652685864);
  - **HTTP server:** `tgd serve` with versioned native routes, GeoJSON, and Nominatim-compatible
    `/search`, `/reverse`, `/lookup` held to a corpus of client requests (D-312, D-314);
    limits, timeouts, CORS off unless configured, a panic answered `internal`; the query
    parsing fuzzed on every push and nightly (`server_query`, D-313); `fast-check` green at `b8c06dd` (run
    43652685760);
  - **hosted-ready seams:** who asks, may they, how much they used (`Identify`, `Authorize`,
    `Meter`, ADR-0025) as traits with open defaults, called by both servers and tested to gate
    and meter (`the_seams_gate_and_meter_and_a_panic_is_answered`); one request core, so a
    hosted tier is a deployment, not a fork (D-310, D-311);
  - **measured:** the A/B that chose the stack (D-309); load gated per pack on the Nepal pack
    and the world baseline, proved on a planted sleep and leak; `tgd` 4.59 MB without the
    servers, 9.38 MB with both (D-320).
- [x] Not blocking M6, recorded:
  - the load gate runs locally before a release, not in CI, until CI has real packs (a data
    release); its floors are loose because the machine is shared ([load](../../02-research/notes/server-load.md) §7);
  - the conformance scenarios that need the SDK's fixture tools are listed, not scored
    ([conformance](../../02-research/notes/mcp-conformance.md) §7);
  - a large country's boundary is the slowest route (5k to 10k requests per second on Nepal),
    bounded by the outline's size; no hosted deployment exists, by design (vision).

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Phase 6 drafted from the servers research | M5 met (D-308) |
| 2026-11-15 | 6.0 done: the HTTP A/B, axum with inline engine calls (D-309) | measured |
| 2026-11-15 | 6.1 done: the request core (D-310, D-311); the panic guard moves to 6.2's handler | built |
| 2026-11-15 | 6.2: native routes and `tgd serve` (D-312); fuzzing split into its own item | built |
| 2026-11-15 | 6.3: the MCP server on stdio (D-315); `hierarchy` and `children` join the request core; tools named in `snake_case`; the text block is the full JSON, not a summary; the remaining ADR-0017 tools split into their own item | built |
| 2026-11-15 | 6.3: MCP at `/mcp` of the HTTP server, stateless, `Host` and `Origin` checked (D-316) | built |
| 2026-11-15 | 6.3: `boundary` and `distance` in the request core, HTTP and MCP (D-317); `coverage` moves beside downloads, since both read the catalog | built |
| 2026-11-15 | 6.3: coverage and downloads researched ([note](../../02-research/notes/mcp-downloads.md)); one job registry answers both `tasks/get` and `install_status`; install opt-in on HTTP (D-318) | planned |
| 2026-11-15 | 6.3: `coverage` built: `tgd locate`, `/v1/coverage` and the MCP tool share one function and a read-only catalog read (D-318) | built |
| 2026-11-15 | 6.3: downloads built: `install` and `install_status` over one job registry, as a task or a handle; the size confirmed by MRTR, by `elicitation/create` on stdio (`Mcp::duplex`), else `max_bytes`; `tgd mcp --no-install`, `tgd serve --allow-install` (D-318) | built |
| 2026-11-15 | 6.3: the MCP conformance suite in CI for both revisions; its first run found three defects, fixed (own origin, undeclared methods, `data.uri`) (D-319) | built |
| 2026-11-15 | 6.4 done: the load harness and per-pack budgets, `tgd` sizes per feature set; boundary picks its level by count (D-320) | measured |
| 2026-11-15 | 6.5: M6 met, each criterion with its evidence (D-321) | exit review |
