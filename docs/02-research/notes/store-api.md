---
title: "The store in every language: a shared client, jobs, progress and cancel"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# The store in every language: a shared client, jobs, progress and cancel

## 1. Question

Phase 5.1 puts the store (init, refresh, catalog, find, locate, install, update, remove, pin,
repair, status) into every binding, with long work as jobs (D-274,
[bindings §6](../../04-architecture/bindings.md#6-handles-threads-and-jobs-d-274)). Where does
the orchestration live, and how do progress and cancellation reach a job without changing the
verified installer?

## 2. Summary and recommendation

- **One client, in `teigeodata-sync`** (feature `client`): the orchestration the CLI holds today
  (`crates/teigeodata-cli/src/sync.rs`, 877 lines: the store's config and trust anchor, refresh
  with the countries installed, the selection rules of an install and an update, dependencies
  first, the platform downloads pass, repair, status) moves there with typed results and no
  printing. The CLI formats those results; the API's `Store` handle wraps the same client. One
  code path (DRY, §11), and the CLI's integration tests guard the move.
- **The client stays engine-free**, as `teigeodata-sync` is today: `find` takes the catalog's
  `Resolver`, which the engine implements; opening an engine over the installed packs (each
  with its components and parts) is the API's job.
- **Progress and cancel come from a transport wrapper**, not from changes to the installer: the
  job's transport checks the cancel flag before every request and inside the streaming sink,
  and counts the bytes each frame brings. A cancelled request fails with a new
  `FetchError::Cancelled`; the store commits last (D-171), so a cancelled install leaves the
  journal as it was, the same path a cut connection already takes and is tested for.
- **A job is a thread Rust owns** (D-274) with shared state behind a `Mutex` and a `Condvar`:
  `state`, `progress` (done, total, seq), `wait(seq, timeout_ms)`, `cancel`, `result`. One store
  runs its jobs one at a time: the job holds the store's lock for its whole run.

## 3. Findings

- The CLI's `Opened` (config, shipped root, mirrors, downloads, pending) and its functions are
  the whole policy; `teigeodata-sync` holds the mechanisms (refresh, install, the store, the
  journal). Nothing in the policy depends on the terminal except printing and stderr warnings,
  which become result fields (damaged packs on open, catalog entries that failed to parse, the
  fallback to the stored catalog when the network fails).
- `Transport::get_streamed` hands the body to a sink frame by frame (D-168); a wrapper sees
  every byte without touching the installer.
- The catalog states each asset's length, so a job's total is known before its first request.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Client in `teigeodata-sync`, CLI and API on top | one policy; testable without a terminal; the CLI tests guard it | a refactor of the CLI first | **chosen** |
| The API calls the CLI's code | no move | the CLI is a binary crate; the API would pull in clap and printing | rejected |
| A second copy in the API | fast to write | two policies drift; against DRY (§11) | rejected |
| Progress by callback from the installer | precise per section | foreign code on a Rust thread (D-274 forbids); installer changes | rejected |

## 5. What we build ourselves, and why

The client (moved, not new), the transport wrapper (about 60 lines), and the job (a thread, a
mutex, a condvar). Gates: the CLI's sync tests unchanged; a job test that cancels mid-install
and finds the journal as it was; progress that only moves forward and ends at the total;
`wait` that returns at once on a finished job.

## 6. Impact on the plan

Phase 5.1's sync item is built in three commits: (1) the client moved into `teigeodata-sync`,
the CLI on top; (2) jobs, progress and cancel in the client; (3) the `Store` and `Job` handles
in `teigeodata-api` and every binding.

Built as planned (D-279), with three findings: the names `init` and `wait` collide with
Swift's initialiser and Java's `Object.wait`, so the API says `create` (which also opens) and
`wait_progress`; the HTTP and TLS stack more than triples the C library (949 KB to 3.1 MB,
`dist`), so the store is a default-on feature that the IDL carries to every generated layer;
and the transport had never used the platform verifier it linked, so it keeps Mozilla's roots
and drops it.

## 7. Risks and open items

- A job's thread holds the store lock; a second job waits. A long install blocks `status`
  until it ends: acceptable for v1 (the engine stays free), revisit if apps need status during
  an install.
- Platform downloads (D-184) stay a CLI and platform flow; the API does not expose the pending
  ranges yet (5.1 left).
- `result` and `wait_progress` block the calling thread. On Node's main thread and Dart's UI
  isolate an app must call them off the event loop until each language's idiomatic layer
  (bindings §6: a `Promise` over an `AsyncTask`, a helper isolate) exists.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| ureq | 3.5.1 | 2026-11-14 | `crates/teigeodata-sync/Cargo.toml` |

## 9. Sources

- D-274 and [bindings §6](../../04-architecture/bindings.md#6-handles-threads-and-jobs-d-274).
- D-168 (transport streaming), D-171 (atomic install, journal), D-184 (platform downloads).
