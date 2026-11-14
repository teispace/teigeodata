---
title: "Coverage and downloads in the servers"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2026-12-15
---

# Coverage and downloads in the servers

## 1. Question

Phase 6.3 has two items left before the conformance suite: `coverage` (which packs hold a
point) and downloads from MCP (install a pack as a task where the client has the tasks
extension, a handle and a status tool otherwise, the size confirmed by elicitation). How does
rmcp 3.10.0 carry tasks and elicitation for both revisions, what does an install need from the
store, and how is a tool that writes to disk and reaches the network kept safe on a server?

## 2. Summary and recommendation

- **`coverage`** is `tgd locate` as a query: the countries at or near a point (the engine's
  locator) and, for each, the catalog's packs that could hold it with their release, sizes and
  state (installed, update, available). One function in `teigeodata-server` (feature `store`)
  answers `tgd locate`, `/v1/coverage` and the MCP tool, so the three agree. It reads only the
  stored, verified catalog: offline, read-only.
- **`install`** installs catalog packs into the server's store through the store's client
  (`teigeodata_sync::Client::install`), on a blocking thread, one install at a time (the store
  is one writer). The served packs follow the store's journal, so a finished install is served
  without a restart; the server reloads at once after its own install rather than waiting for
  the next follow.
- **One job registry for both paths** (our design, §5): a job has a random 128-bit ID, a
  state (working, completed, failed, cancelled), a status message and its report. A client
  that declared the tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663) gets a
  `CreateTaskResult` and polls `tasks/get`, which the server answers from the registry; any
  other client gets the same ID as a `handle` in a normal result and polls the
  `install_status` tool. rmcp's `TaskManager` is not used: it would keep a second registry
  beside the one `install_status` needs.
- **The size is confirmed** before any byte is fetched. A 2026-07-28 client that declared
  form elicitation gets an MRTR `input_required` result (SEP-2322) with one `ElicitRequest`
  ("Download 80.2 MB to install np/places?"), and `requestState` sealed with the server's
  BLAKE3 key as the cursors are; the retry carries the answer and the state. A 2025-11-25
  client on stdio that declared elicitation gets a server-to-client `elicitation/create`. A
  client with neither must pass `max_bytes` at least the download (default 100 MB): the model
  sees the size in the refusal and the host's approval of a non-read-only tool is the person's
  confirmation.
- **Exposure.** The three tools exist only on a server over a store (not `--pack` alone).
  `install` and `install_status` are on by default for `tgd mcp` (a local agent, the person's
  own store) and off for `tgd serve` unless `--allow-install`; seams gate them as their own
  operations (`Coverage`, `Install`). `install` is annotated `readOnlyHint: false`,
  `destructiveHint: false`, `idempotentHint: true`, `openWorldHint: true`.

## 3. Findings

**Tasks (rmcp 3.10.0, `src/model/task.rs`, `src/handler/server.rs`; checked 2026-11-15).**
The extension ID is `io.modelcontextprotocol/tasks`. A server returns `CallToolResponse::Task`
(`resultType: "task"`) from `call_tool`; rmcp refuses to send it unless the request declared
the extension (`ClientCapabilities::supports_tasks`), so the handler must check the same flag
and answer normally otherwise. `tasks/get` returns a `DetailedTask` (`Task` plus a payload:
`working`, `input_required` with `inputRequests`, `completed` with the original result,
`failed` with a JSON-RPC error, `cancelled`); `tasks/update` answers in-task input requests;
`tasks/cancel` is cooperative. `tasks/list` is gone in 2026-07-28. rmcp gates the three
methods on the server advertising the extension and the client declaring it. `Task` carries
`ttlMs` (the server may drop the task after it) and `pollIntervalMs`.

**MRTR (`src/model/mrtr.rs`).** A 2026-07-28 server does not send requests to the client:
`tools/call` answers `InputRequiredResult` (`inputRequests` keyed by our names, each an
`ElicitRequest`, a sampling or a roots request; `requestState`, opaque to the client) and the
client retries with `inputResponses` and the echoed state. rmcp only lets it reach a peer that
negotiated 2026-07-28. `requestState` is untrusted: seal it (rmcp's `request-state` feature
uses HMAC-SHA256 and base64; we already sign cursors with a BLAKE3 keyed hash and use that).

**Elicitation for 2025-11-25.** A server-to-client `elicitation/create` over the session.
Stateless HTTP answers JSON per request with no server-to-client channel (D-316), so it is
possible on stdio only; over HTTP an older client falls back to `max_bytes`.

**The store (`teigeodata-sync`, feature `client`).** `Client::install(paths, want, pin,
downloads)` installs dependencies first, refreshes the catalog (falling back to the stored
one), and returns a `Report` (`done`, `pending`, `stale_catalog`). It has no progress callback
and cannot be interrupted mid-pack; an install resumes from its staging file. `Entry::size_of`
gives the wire and disk bytes for a set of components. `tgd locate` (CLI) builds its rows
from the engine's `countries_near` and `Catalog::packs_at`. The CLI already depends on the
`client` feature, so `tgd` does not grow; the server crate gains it behind a feature.

**Safety.** The MCP security guidance and the 2025-11-25 tool annotations: hosts ask the
person before a tool that is not read-only. A network-facing server writing to its disk on a
request is a new capability for a deployment, so it is opt-in there (as CORS is, D-312).

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| rmcp `TaskManager` for tasks, our registry for handles | Less code for tasks | Two registries; `install_status` cannot see tasks | No |
| **One registry, `get_task`/`cancel_task` answered from it** | One state, both paths agree, bounded | We write the `DetailedTask` mapping | **Yes** |
| Install inline, no handle | Simplest | Blocks a call for minutes; HTTP times out at 10 s | No |
| Confirm by `max_bytes` only | Works everywhere | The person never sees the size when the client could ask | Fallback only |
| MRTR elicitation (2026-07-28), `elicitation/create` (2025-11-25 stdio) | The person confirms the size | Two code paths | **Yes, with the fallback** |

## 5. What we build ourselves, and why

The job registry: at most 16 jobs kept, each dropped one hour after it ends (`ttlMs`
3,600,000), `pollIntervalMs` 1,000; one install runs at a time and a second waits its turn
(working, "waiting for another install"). Cancelling a waiting job removes it; cancelling a
running one marks it cancelled once the pack in progress is in (the store stays consistent).
Gates: a signed local repository (the sync tests' fixture) served on localhost, an install by
task and by handle, the confirmation on each path, a refusal past `max_bytes`, a forged
`requestState`, the tool absent without a store and on `tgd serve` without
`--allow-install`, and `coverage` equal to `tgd locate`.

## 6. Impact on the plan

Phase 6.3's `coverage` item joins the downloads item; the status tool is named
`install_status` (tools are `snake_case`, D-315). Decision D-318.

## 7. Risks and open items

- No progress inside a pack: the status message names the pack, not its bytes. A progress
  hook in `Client::install` is a later improvement.
- A server run as several processes behind a balancer would need the registry shared; not a
  v1 deployment (one process per store).

## 7a. What building it taught (2026-11-15)

- rmcp's `Peer::create_elicitation` needs its `elicitation` feature, which brings `url` and
  25 crates of ICU tables. The server sends the same request through the peer's generic
  `send_request_with_option` instead, with no new dependency. The person has 10 minutes to
  answer; no answer installs nothing (`unavailable`).
- Whether the server may ask the client is a property of the transport, not of the client:
  `Mcp::duplex` marks a server on stdio (`serve_stdio` sets it). Stateless HTTP answers each
  POST alone, so a 2025-11-25 client there gets the `max_bytes` bound, as planned.
- A call asked about over the session is planned again with the answer, so the size the
  person confirmed is the size of what runs, and the seams admit it again (they are
  stateless: identify and authorise).
- Gates (all proved on a planted fault): the confirmation asked on each path and declined in
  three ways; a forged `requestState`; `max_bytes` without a duplex transport; the task
  polled to completion; the tools absent without a store, on `tgd mcp --no-install` and on
  `tgd serve` without `--allow-install`; `tgd mcp` asking a 2025-11-25 client end to end.

## 8. Versions checked

| Item | Version | Date | Where |
|---|---|---|---|
| rmcp | 3.10.0 | 2026-11-15 | `cargo search rmcp` |
| MCP | 2026-07-28 (fallback 2025-11-25) | 2026-11-15 | [servers](servers.md) §3 |

## 9. Sources

- rmcp 3.10.0 source: `src/model/task.rs`, `src/model/mrtr.rs`, `src/task_manager.rs`,
  `src/handler/server.rs`, accessed 2026-11-15.
- SEP-2663 (tasks extension) and SEP-2322 (MRTR), as summarised in
  [bindings and MCP](bindings-and-mcp.md) §3.13 and implemented by rmcp.
- `teigeodata-sync` `Client::install` and `Catalog::packs_at`; `tgd locate`.
