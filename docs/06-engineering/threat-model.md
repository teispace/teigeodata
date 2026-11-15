---
title: "Threat model"
status: accepted
updated: 2026-11-15
---

# Threat model

Each trust boundary in [`SECURITY.md`](../../SECURITY.md)'s scope gets three things: what an
attacker controls there, what must hold, and the test or gate that shows it does. Phase 8.2
wrote this page ([plan](../07-planning/phases/phase-8.md), D-331). A row marked **open** is a
gap the review found and has not closed yet. Data errors, such as a wrong position or name, are
not security issues.

The `unsafe` code under these boundaries is listed block by block, each with its argument, in
the [unsafe inventory](unsafe-inventory.md), which `cargo xtask unsafe-audit` writes.

## The promise

**Untrusted input never causes memory unsafety, a panic across the FFI, or unbounded memory
or CPU use.** That covers pack bytes, the catalog and its trust metadata, the network, and
every caller of the C ABI or the servers. An untrusted input gets a typed error or an answer.
Panics count too: the library never panics on input (P6, the workspace lints `unwrap_used`,
`expect_used`, `panic` and `indexing_slicing`).

## 1. Pack bytes

A pack may come from anywhere: a mirror, a file someone sent, a corrupted disk.

- **The attacker controls** every byte. That includes the footer, the section table, offsets
  and lengths. With the writer, it also includes contents whose hashes are right.
- **What must hold:**
  - Opening checks the container: magic, versions, the footer and the section table, every
    range in bounds. Each section's BLAKE3 hash is checked before it is read, and a signed
    pack's Ed25519 signature before the manifest is trusted.
  - Each section's `open` checks everything a query relies on, in O(grids or columns). The
    full check (`check`, `tgd verify --deep`) adds the O(rows) checks. Nothing a query does
    can index, shift or allocate past what `open` checked (D-332).
  - No input makes a query allocate without bound or loop without end.
- **Shown by:**
  - **Fuzzing:** `open`, `round_trip`, `mutate`, `manifest`, `sections`, `bitpack`, `table` and
    `bits` (container and codecs). `engine` asks every query of a pack with one or two
    sections edited and its hashes right, and runs the edited sections' full check. Nightly
    runs each target for 300 s, under a 2 GB RSS limit and a 10 s per-input timeout. The weekly
    campaign runs them for 3 h, and its gate fails unless every section kind's decoder is
    reached ([ci.md](ci.md#fuzzing-campaign)).
  - **Miri:** the reader and codec tests (nightly `miri`).
  - **Tests:** the seeded harnesses in each crate's `tests/fuzz.rs`, and every crash input
    kept in `found_by_libfuzzer`.
  - **Faults found and fixed:** two reader faults the `engine` target found on its first run
    (D-332).
- **Residual:** a mapped file truncated by another process raises `SIGBUS` on POSIX, which no
  safe library can catch. The store never writes over or truncates an installed pack, a file
  outside the store is read rather than mapped, and `Layer::unchanged` sees a replaced file
  (`crates/teigeodata-engine/src/map.rs`).

## 2. The catalog, trust metadata and the update chain

- **The attacker controls** the mirror and the network path: any metadata file, any asset,
  old signed files replayed, and endless responses. A stolen online key (timestamp or
  snapshot) is in scope. A stolen offline root key is not, since it is the ceremony's
  (phase 4.3).
- **What must hold:**
  - The TUF chain (root, timestamp, snapshot, targets with per-country delegations) is
    verified before any catalog entry is used.
  - Rollback, freeze, mix-and-match and endless data are refused, and a root rotates only
    with both thresholds.
  - Every downloaded byte is checked against the signed catalog: the asset's hash, the
    pack's head hash and each frame's block hashes. A resumed or partial install is checked
    the same way.
  - An install interrupted at any point leaves the store whole.
  - A signed catalog still cannot name a path outside the store.
- **Shown by:**
  - The 15 [sync gates](sync-gates.md), with rollback, freeze, mix-and-match, endless data,
    root rotation, the python-tuf and tough differentials, crash at every step, resume and
    bad mirrors.
  - The `trust` fuzz target, which attacks each role in turn (replaced, edited and re-signed,
    or signed by an unknown key) and rebuilds the roles after it so their checks are reached.
  - The `transport_table`, `transport_asset` and `transport_frame` targets.
  - `catalog::tests::an_asset_name_never_leaves_the_store`, for names that climb out, nest,
    hide or are not plain ASCII. The review found this check untested and added the test,
    proved red with the `..` check removed.
- **Residual:** confidentiality. TUF protects integrity whatever the transport is, but a
  network observer can see which packs a client fetches. The default transport is HTTPS
  (rustls); a mirror over plain HTTP gives up that privacy, never integrity.

## 3. The C ABI

- **The attacker controls** every argument a host program passes, which may itself come from
  untrusted input: null pointers, lengths that lie, text that is not UTF-8, any number, any
  option record, a callback that cancels or panics.
- **What must hold:**
  - No panic or unwind crosses the boundary: every export runs in `abi::call`, which stops a
    panic and returns `TG_ERROR_INTERNAL`.
  - Each pointer is checked for null before use, each length bounds its read, and each text
    is checked as UTF-8.
  - Each buffer the library returns is freed exactly once by `tg_bytes_free`.
  - The ABI changes only by the rules of [abi.md](../04-architecture/abi.md).
- **Shown by:**
  - `tests/abi.rs`, and `abi::tests::a_panic_stops_at_the_boundary`.
  - The `ffi` fuzz target: all 33 engine functions with fuzzed arguments, failing on a stopped
    panic. `the_harness_calls_every_engine_function` fails when the ABI gains a function the
    harness does not call.
  - The ABI gate `cargo xtask abi` against the last tag (D-285).
  - The C, C++, Go, Swift, Dart and .NET parity suites.
  - The `unsafe` inventory.

## 4. The language bindings

- **The attacker controls** what a host program's own untrusted input passes in: any value of
  the host language, and any order of calls, close and dispose included, from any thread.
- **What must hold:** no host value reaches the library unchecked. Unsigned ranges are
  checked where the host has no unsigned type (JVM). A handle used after close is an error
  and never a dangling pointer: Go uses `RWMutex`, .NET `SafeHandle`, the JVM a read lock and
  a reachability fence. An exception or panic in a callback is carried to the caller.
- **Shown by:** each binding's parity suite over `parity.json`, with closed-handle and
  concurrent-close tests where the language has threads. `parity-mutants` plants every value
  of `parity.json` in every language. There are fuzzers in Go (`FuzzReader`) and .NET (50,000
  seeded blob mutations) (D-297, D-298, D-300).

## 5. The HTTP and MCP servers

- **The attacker controls** every request: the path, the query string, the headers, the
  `/mcp` body, how many requests arrive at once, and how slowly they are sent. A page in a
  browser can also try DNS rebinding against a server on loopback.
- **What must hold:**
  - Every query keeps its bounds (`Limits::SERVER`): 256 bytes of text, 50 results, a
    1,000 km radius and 20,000 outline vertices.
  - A query string is at most 4 KiB (414 past it), a `/mcp` body at most 64 KiB (413), and a
    request is answered within 10 s (503 past it).
  - Floods and slow clients are bounded (D-334). At most 1,024 connections are open; more wait
    in the kernel's backlog. At most 256 requests are answered at once, and one past it is
    refused at once (503, `Retry-After`). A request head must arrive within 10 s, which also
    closes an idle HTTP/1 connection. HTTP/2 peers are pinged and get at most 32 streams.
  - A panic answers `internal` and never ends the process.
  - CORS is off unless configured.
  - `/mcp` refuses a foreign `Host` (rebinding) and any `Origin` not allowed, and holds no
    session state.
  - The seams (authorize, meter) see every request before it is answered.
- **Shown by:**
  - `tests/query.rs::every_bound_holds_at_its_edge_and_refuses_past_it`.
  - `tests/http.rs::every_failure_answers_its_code_and_status`, which includes the 414.
  - `tests/http.rs::the_seams_gate_and_meter_and_a_panic_is_answered`.
  - `tests/mcp_http.rs::mcp_refuses_a_foreign_host_and_origin`.
  - The `server_query` and `mcp` fuzz targets.
  - `tests/limits.rs`, one test per bound, each red with its bound removed: in flight, the
    `/mcp` body, a body that never ends, a slow head and an idle connection, the connection
    cap.
  - The load gate (`cargo xtask load`, D-321), within its budgets with the bounds in place.
  - The MCP conformance suite (`mcp-conformance.yml`).
- **Residual:** a query runs to its end after its timeout. Its cost is bounded by `Limits`
  and measured by the load gate. TLS and per-client rate limits belong to a reverse proxy or
  the seams ([server limits](../02-research/notes/server-limits.md)).

## 6. The local store and files

- **The attacker controls** a file placed where the library reads it (a pack someone sent),
  and other processes on the machine.
- **What must hold:**
  - A file outside the store is read, not mapped.
  - The store writes each pack once under a new name and renames it into place, never writing
    over a pack that may be mapped. Its journal commits whole or not at all.
  - Every name a file gets comes from checked catalog values (§2).
  - A punched hole frees disk without changing what a read sees (D-189).
- **Shown by:** sync gates 8, 9 and 15. `holes::tests` runs on APFS, ext4 and NTFS (fast-check
  and its `windows` job).

## 7. The build and release pipeline

- **The attacker controls** a pull request from anyone: its code and its workflow edits. An
  upstream dependency or action could also be compromised.
- **What must hold:**
  - A pull request never runs with secrets or write permission. `pull_request_target` is not
    used with untrusted code, and permissions are least-privilege per workflow.
  - Every action is pinned by commit SHA, and Renovate updates them. Every crate source and
    advisory passes `cargo deny`.
  - A release is built by `release.yml` from a dispatched commit, approved in the `release`
    environment. Each file has a CycloneDX SBOM, build provenance attested through Sigstore,
    and each attestation is verified before publishing (D-302 to D-305).
  - Commits are signed off (DCO), and the hooks refuse what the rules forbid.
- **Shown by:**
  - `cargo deny` (fast-check).
  - `zizmor` on every workflow (fast-check): findings fixed, or ignored inline with a reason.
  - CodeQL on Rust, the workflows, JavaScript, TypeScript and Python (`codeql`).
  - OpenSSF Scorecard weekly. Its Vulnerabilities check reads every lockfile, npm included.
  - The release dry run, and the attestation verification in `release.yml`.
  - `cargo-semver-checks` and the ABI gate on release
    ([what 1.0 promises](semver.md)).
- **Residual:** the checks only the maintainer can turn on: branch protection, required
  review, and signed releases from the first tag.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Written: seven boundaries, their gates and the open gaps | D-331, phase 8.2 |
| 2026-11-15 | §5: the server's flood and slow-client bounds, each with its test | D-334 |
| 2026-11-15 | §7: zizmor, CodeQL and Scorecard are gates; the open rows closed | phase 8.2 |
