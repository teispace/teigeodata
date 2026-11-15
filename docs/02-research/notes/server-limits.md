---
title: "Server resource limits: connections, requests in flight, slow clients"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2026-12-15
---

# Server resource limits: connections, requests in flight, slow clients

## 1. Question

The [threat model](../../06-engineering/threat-model.md) §5 found three gaps in `tgd serve`,
the HTTP and MCP server of `teigeodata-server`. Two bounds have no test: the 64 KiB `/mcp` body
and the 10 s timeout. Nothing bounds the requests in flight. Slow clients are bounded only by
the transport's defaults. What bounds should a server that strangers can reach hold, how should
it enforce them, and how is each one proved?

## 2. Summary and recommendation

- **The server sets its own connection limits instead of using `axum::serve`.** axum 0.8.9's
  `serve` builds each hyper connection with no timer. hyper turns on its header-read timeout
  only when a timer is set, so it never fires. A client that sends its request head one byte a
  minute, or keeps an idle keep-alive connection open, holds a task and a socket for as long
  as it likes.

  `http::serve` now runs the accept loop over `hyper_util`'s `auto::Builder` with
  `TokioTimer`, which every axum release already depends on. It sets:
  - a **header-read timeout** (`header_timeout`, 10 s). hyper starts it whenever it waits for a
    request head, so it bounds slow heads and idle HTTP/1 connections alike;
  - **HTTP/2 keep-alive pings** (every 20 s, with a 20 s deadline), which drop a dead peer;
  - at most **32 streams** per HTTP/2 connection;
  - a **connection cap** (`max_connections`, 1,024). At the cap the loop stops accepting, so
    new connections wait in the kernel's backlog instead of using memory.

  `ConnectInfo` and graceful shutdown behave as before.
- **Requests in flight are capped, and a request past the cap is refused at once**
  (`max_in_flight`, 256): `503 unavailable` with `Retry-After: 1`. tower's `ConcurrencyLimit`
  is wrapped in `LoadShed`, so a flood is answered in microseconds instead of queueing until
  every request times out.

  Queries stay on the runtime's workers. They are bounded by `Limits` and take microseconds to
  a few milliseconds ([load gate](server-load.md)). Moving them to `spawn_blocking` would let
  the timeout answer but leave the query burning CPU, and it adds a thread hop to every
  request (option C below).
- **A test per bound**, each proved red:
  - the `/mcp` body past 64 KiB is answered 413;
  - a body that never ends is answered 503 after the timeout;
  - a request past `max_in_flight` is answered 503 with `Retry-After`;
  - a head sent slower than `header_timeout` has its connection closed, and so does an idle
    keep-alive connection;
  - a connection past `max_connections` is not served until one closes.
- **`tgd serve` gains `--max-connections` and `--max-in-flight`.** The defaults suit one
  machine; an operator behind a reverse proxy may raise them.

## 3. Findings

- **axum 0.8.9 `serve`** (`src/serve/mod.rs`) creates
  `hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())` per connection. It sets
  only HTTP/2's CONNECT protocol, with no timer and no HTTP/1 option, and offers no way to
  pass a builder. axum's own example for custom connection settings
  (`examples/serve-with-hyper`) writes the accept loop.
- **hyper 1.12.0**, HTTP/1 (`server/conn/http1.rs`, `proto/h1/conn.rs`):
  - `header_read_timeout` defaults to 30 s, but `Dur::Default` is dropped when no timer is set
    (`check(...)` returns `None`).
  - With a timer, the deadline is armed in `poll_read_head` whenever the connection can read a
    head. That is `Reading::Init`, which is also the state of an idle keep-alive connection.
    So one setting bounds slowloris heads and idle connections.
  - The read buffer is at most 408 KiB (`DEFAULT_MAX_BUFFER_SIZE`); 100 headers by default.
- **hyper 1.12.0**, HTTP/2 (`proto/h2/server.rs`):
  - 200 concurrent streams, a 16 KiB header list and 1 MiB windows;
  - no keep-alive interval (`None`), so an idle or dead HTTP/2 peer is never pinged;
  - the h2 crate's own bound on pending reset streams, 20, against rapid reset
    (CVE-2023-44487).
- **tower 0.5.3:** `ConcurrencyLimit` reports not ready when its semaphore is empty, and
  `LoadShed` turns not-ready into an immediate `Overloaded` error. Axum maps that to a response
  through `HandleErrorLayer`. This is the pattern axum's documentation gives for load shedding.
- **The body is already bounded by time:** `TimeoutLayer` (tower-http 0.7.1) wraps the whole
  service future. rmcp reads the `/mcp` body inside it (`max_request_body_bytes`, 64 KiB), so
  a body sent slowly is answered 503 at the timeout. The `/v1` and Nominatim routes read no
  body.
- **Nominatim and Photon** leave connection limits to the web server or proxy in front of
  them (Apache or nginx, Jetty). A Rust server that strangers reach directly has to set them
  itself.

## 4. Options compared

**Slow clients and connections:**

| Option | Strengths | Weaknesses | Licence | Version | Verdict |
|---|---|---|---|---|---|
| A. Own accept loop over `hyper_util` `auto::Builder` with `TokioTimer`, a connection semaphore, graceful shutdown | Every hyper bound is reachable; no new crate (axum depends on it); bounded per connection and in count | About 80 lines replacing one call; ConnectInfo inserted by hand | MIT | hyper-util 0.1.21, hyper 1.12.0 | **Chosen** |
| B. `axum::serve`, only tower layers | No new code | Cannot set the header timeout or HTTP/2 pings: slowloris stays open | MIT | axum 0.8.9 | Rejected |
| C. Leave it to a reverse proxy | The usual deployment | `tgd serve` is also run directly; the library must hold on its own | — | — | Documented as advice, not relied on |

**Requests in flight:**

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A. `ConcurrencyLimit` inside `LoadShed`, 503 with `Retry-After` | Fails fast; memory bounded; the client knows to retry | A burst past the cap is refused, not queued | **Chosen** |
| B. `ConcurrencyLimit` alone (queue) | Nothing refused while the burst is short | Under a flood the queue grows until each request times out; memory grows with it | Rejected |
| C. `spawn_blocking` per query | The timeout can answer a slow query | The query still runs to the end; the blocking pool grows to 512 threads (about 1 GB of stacks); a thread hop on every request | Rejected |

## 5. What we build ourselves, and why

The accept loop (option A), because axum exposes none of these settings. It follows axum's own
`serve-with-hyper` example: accept, insert `ConnectInfo`, serve with upgrades, and watch for
graceful shutdown with `hyper_util::server::graceful::GracefulShutdown`. On an accept error
such as running out of file descriptors it waits a second and goes on, as axum does.

## 6. Impact on the plan

- [Phase 8](../../07-planning/phases/phase-8.md) 8.2, the resource-limits step, is done when
  each bound above has its test.
- [The threat model](../../06-engineering/threat-model.md) §5 moves its open rows to gates.
- `http::Config` gains `header_timeout`, `max_connections` and `max_in_flight`. That is a
  public API addition, recorded in the CHANGELOG.

## 7. Risks and open items

- A query still runs to its end after its timeout. Each query's cost is bounded by `Limits`,
  and the load gate measures it.
- The defaults are for one machine. A hosted tier sets its own through `Config`, and its rate
  limits through the seams (`Authorize`).
- TLS is a reverse proxy's job. `tgd serve` speaks plain HTTP on loopback by default.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| axum | 0.8.9 | 2026-11-15 | crates.io (`cargo info`), its source |
| hyper | 1.12.0 | 2026-11-15 | crates.io, its source |
| hyper-util | 0.1.21 | 2026-11-15 | crates.io, its source |
| tower | 0.5.3 | 2026-11-15 | crates.io |
| tower-http | 0.7.1 | 2026-11-15 | Cargo.lock |

## 9. Sources

- axum 0.8.9 source, `src/serve/mod.rs`; axum's `examples/serve-with-hyper` and the
  `error_handling` documentation (load shedding with `HandleErrorLayer`).
- hyper 1.12.0 source: `src/server/conn/http1.rs` (`header_read_timeout`, timer check),
  `src/proto/h1/conn.rs` (`poll_read_head`), `src/proto/h1/io.rs` (buffer bound),
  `src/proto/h2/server.rs` (HTTP/2 defaults).
- hyper-util 0.1.21 documentation: `server::conn::auto::Builder`, `rt::TokioTimer`,
  `server::graceful::GracefulShutdown`.
- tower 0.5.3 documentation: `limit::ConcurrencyLimit`, `load_shed::LoadShed`.
- CVE-2023-44487 (HTTP/2 rapid reset) and the h2 crate's `max_pending_accept_reset_streams`.
- This repository: `crates/teigeodata-server/src/http.rs`, `src/mcp.rs` (`MAX_BODY_BYTES`),
  [server-load](server-load.md).
