---
title: "The store in the browser"
status: accepted
updated: 2026-11-17
owner: teigeodata maintainers
revisit: 2027-05-17
---

# The store in the browser

## 1. Question

How does `teigeodata-wasm` install, update and repair packs in a browser, with the same trust
chain as the native store: download with `fetch` and range requests from a mirror, keep packs
across visits, run off the main thread, and answer from a bundled baseline before anything is
downloaded? It feeds TG-25 (D-362) and the browser half of the catalog going live (TG-24).

## 2. Summary and recommendation

**An async driver in a dedicated Worker, over the sans-I/O cores the native store already
uses, storing packs in the Origin Private File System (OPFS).**

- The verifier (`trust::Trusted`) and the installer (`install::Install`) are already sans-I/O
  state machines: the caller fetches the ranges they ask for and writes the bytes they hand
  back. The browser driver calls them from JavaScript-facing wasm exports and does the I/O with
  `fetch` (with `Range` and an `AbortController`) and OPFS. No protocol or check is written
  twice.
- The parts that are not sans-I/O today, the refresh walk (`refresh.rs`) and the store's
  policy (`client.rs`: install order, update, evict, repair), are split into sans-I/O plans
  first, which the native client then drives too. One code path for every platform (§11), and
  the native store loses nothing.
- **Storage: OPFS.** A pack is one file, written at offsets as frames arrive and read at
  offsets when opened. Install goes to `staging/`, then the file is moved into `packs/`; the
  store's index is written to a temporary file and moved over the old one. One worker owns the
  store per origin, elected with the Web Locks API, so two tabs never write at once.
- **The engine** opens the bundled baseline from bytes at once, then reopens with the installed
  packs when an install finishes. Queries run in the worker (or wherever the app opens an
  engine); the store tells it when packs change.
- **Persistence:** ask `navigator.storage.persist()`, and treat the store as a cache anyway.
  Safari deletes an origin's script-written storage after seven days without use, so the
  baseline must always answer, and the store repairs or reinstalls on open.

Rejected: synchronous XHR (allowed in workers, but OPFS's file creation, moves and listing are
async anyway); JSPI (only from Chrome 137, Firefox 153 and Safari 27, so older iPhones are
left out; revisit in 2027); `SharedArrayBuffer` with `Atomics.wait` (needs cross-origin
isolation, which breaks embedding); Cache Storage (refuses 206 responses, no writes at
offsets); IndexedDB (no partial writes; values are copied in and out).

**Unverified until the prototype measures it:** the memory of a large pack in wasm memory on
a phone (Nepal's full pack is 87.5 MB), and OPFS write speed on Safari.

## 3. Findings

### 3.1 What exists

- `teigeodata-wasm` opens packs from bytes (`openBytes`) only; a page fetches a pack itself
  ([node and wasm packaging](node-wasm-packaging.md), §"not in this step").
- `teigeodata-sync`: `install` (feature `install`, `no_std`) and `trust` are sans-I/O: `Install::first`, `start`,
  `feed`, `remaining`, `finish`; `Trusted::next_root_name`, `update_root`, `update_timestamp`,
  `snapshot_name`, `update_snapshot`, `targets_name`, `update_targets`, `delegated_name`. The
  `Fs` and `Transport` traits are synchronous, and `refresh.rs`, `store.rs` and `client.rs` use
  `std` files.
- A mirror is two flat trees of files (metadata and assets); the assets are fetched with
  `Range` and must answer 206. GitHub's release assets send no CORS headers
  ([offline loading](offline-loading-and-trust.md) §3), so a browser needs a CORS-enabled
  mirror.

### 3.2 Browser support (MDN browser-compat-data, read 2026-11-17)

| API | Chrome | Firefox | Safari | Use |
|---|---|---|---|---|
| `navigator.storage.getDirectory()` (OPFS) | 86 | 111 | 15.2 | the store's root |
| `FileSystemFileHandle.createSyncAccessHandle()` and the handle's `read`, `write`, `truncate`, `flush` | 102 | 111 | 15.2 | writes and reads at offsets, in a worker |
| `FileSystemFileHandle.createWritable()` | 86 | 111 | 26 | not needed |
| `FileSystemHandle.move()` (files only) | 102 | 111 | 15.2 | staging → packs, index replace |
| `FileSystemDirectoryHandle.removeEntry()` | 86 | 111 | 15.2 | remove, evict |
| `StorageManager.persist()` / `estimate()` | 55 / 61 | 57 / 57 | 15.2 / 17 | persistence, free space |
| `LockManager.request()` (Web Locks) | 69 | 96 | 15.4 | one store owner per origin |
| WebAssembly JSPI | 137 | 153 | 27 | rejected for now |

The floor is therefore Chrome 102, Firefox 111 and Safari 15.4.

### 3.3 Rules that shape the design

- **Synchronous XHR** is refused only in a `Window`: "If the current global object is a
  Window object and this's synchronous is true, then throw an InvalidAccessError" (XHR Living
  Standard). It is allowed in workers, but it doesn't help: creating a sync access handle,
  moving and removing files are all promise-based.
- **The Cache API rejects partial responses:** `Cache.put` returns "a promise rejected with a
  TypeError" when the response's status is 206 (Service Workers spec), and a cached response is
  read whole.
- **Quotas and eviction** (from [offline loading](offline-loading-and-trust.md) §3.3):
  - The quota is per origin, shared by IndexedDB, the Cache API and OPFS.
  - When storage runs short, browsers evict whole origins, least recently used first.
  - Safari deletes script-written storage after seven days of browser use with no interaction.
  - `persist()` prompts in Firefox; Chromium and Safari decide silently.
- **A sync access handle is exclusive:** while one is open, the file can't be opened again. A
  second tab's worker would fail, hence the Web Lock.

### 3.4 Sizes

Released packs (`data-2026.11.15`):

| Pack | Size |
|---|---|
| `world-baseline-lite.tgd` | 5.03 MB |
| `np-baseline-lite.tgd` | 67.6 kB |
| `np-places-lite.tgd` | 87.5 MB |

An app usually installs parts of a pack (components and languages), which the installer
already supports: the parts not installed stay sparse in the file. TG-26 measured the world
lite pack without boundaries at 11.85 MB.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A. Async driver over sans-I/O cores, OPFS | Every browser from Safari 15.4; `fetch` streaming, progress and abort; one trust and install code path | Refresh and policy must first become sans-I/O plans (a refactor of `refresh.rs` and `client.rs`) | **Chosen** |
| B. Native client unchanged, with JSPI for the async calls | The least new code | Chrome 137, Firefox 153, Safari 27 only; a young toolchain path in wasm-bindgen | Revisit 2027 |
| C. Native client, sync XHR, files through `Atomics.wait` | Sync code reused | Needs `SharedArrayBuffer`, so cross-origin isolation (COOP and COEP) on the consumer's site | Rejected |
| D. An async copy of the client in Rust | Clean async | Two policies to keep equal: against DRY | Rejected |
| Storage: Cache Storage | Simple, built for responses | No 206, no partial writes, read whole | Rejected |
| Storage: IndexedDB | Wide support | No writes at offsets; values copied; a pack as chunks adds an index of its own | Rejected |
| Storage: OPFS | Real files, writes and reads at offsets, `move` | Async open and move; exclusive sync handles | **Chosen** |

## 5. What we build ourselves, and why

1. **Sans-I/O plans in `teigeodata-sync`**: a refresh plan ("fetch this metadata file, up to
   this many bytes"; feed it the bytes or "missing"), and a store plan (what to install,
   update, evict or repair, from the catalog and the installed packs). The native client
   drives both with its sync `Fs` and `Transport`; its tests stay as they are and must pass
   unchanged.
2. **wasm exports** (`teigeodata-wasm`, a `store` feature so apps that don't use it don't pay
   for it): the plans, `Install`, and the index format.
3. **A worker module** in the npm package, `teigeodata-wasm/store-worker`:
   - opens OPFS and takes the Web Lock;
   - drives the plans with `fetch` and OPFS;
   - answers `init`, `refresh`, `find`, `locate`, `install`, `update`, `remove`, `pin`,
     `repair` and `status` as messages, with progress events;
   - hands an engine the installed packs.

   The API keeps the shape of the native `Store` and `Job`.
4. **Baseline first:** the engine opens the bundled baseline from bytes and adds installed
   packs as they land. The store never blocks the first query.

**What the prototype must prove:**
- A lying mirror is refused and the next one resumes, as in the native tests.
- An install killed partway (the tab closed) resumes or rolls back on the next open.
- Memory and time to open: Nepal's lite pack and the TG-26 app pack, in Chrome and Safari.
- Two tabs never write at the same time.

### 5b. Embedding: the store writes, any wasm reads (added 2026-11-17)

An app may hold its engine inside its own wasm (a host core built on `teigeodata-engine`), so
the store must not bring a second engine. The contract, option (a) of the consumer's review:

- **The store worker only writes.** It owns `staging/`, `packs/` and the index, and holds the
  store's Web Lock. It never needs an engine of its own.
- **Installed files never change.** Each installed pack is a file named by its content
  (`<address>.<component>.<release>.<hash8>.tgd`). An update installs a new file, so a reader's
  file never changes under it.
- **The index is the commit.** It lists the installed packs with each one's file name, length,
  head hash and installed parts, plus a generation number. It is written to a temporary file
  and moved over the old one. A reader sees the old set until that move, then the new one.
- **Readers announce themselves.** The store posts `{generation}` on a `BroadcastChannel`
  after each swap. A reader holds a *shared* Web Lock named for the generation it has open, and
  takes the new one before releasing the old. The store deletes a generation's unused files
  only once it can take that generation's lock *exclusively*, so files in use are never
  deleted.
- **Reading is not exclusive.** A reader opens a pack with `getFile()` (Chrome 86, Firefox
  111, Safari 15.2) and reads ranges with `Blob.slice().arrayBuffer()`. A sync access handle
  locks the file, and the shared modes are Chrome 121+ only (MDN, 2026-11-17).
- **Lazy by section.**
  - A reader reads the head first, checks it against the head hash in the index, then reads
    only the sections of the components it opens, each checked against the head's BLAKE3 as it
    arrives. Bytes outside those sections never enter memory.
  - This needs an engine entry that opens a pack from its head and a set of section buffers.
    Today's `open_bytes` takes the whole file; the store's sparse files already allow the
    rest. Per-query page faults are not possible this way, since reads are async; Chrome's
    `read-only` sync handles could add that later.
- **What the npm package gives a host:**
  - the store worker;
  - a small reader module (index, generation lock, head check, section reads) that hands
    verified buffers to whichever engine the host holds;
  - the index format, documented and versioned.

  A host in Rust can use the same reader logic from `teigeodata-sync` (the index and the
  checks are `no_std`).

### 5c. Plans as one async core (added 2026-11-22, D-369)

§5 asked for sans-I/O *plans*: state machines the caller feeds ("fetch this file; here are
the bytes"). Writing the refresh walk that way (root rotations, the committed snapshot as a
floor, every held role, mirror fallback with demotion) means a second copy of its control
flow as explicit states, where a missed transition is a silent bug. Compared:

| Option | One copy of the logic | Native path | Browser path | Runtime deps |
|---|---|---|---|---|
| Hand-written plans (state machines) | No: the walk is re-expressed as states | feeds the plan from `Fs`/`Transport` | feeds it from `fetch`/OPFS | none |
| **One `async` core over an I/O trait** | **Yes** | **`block_on` with a no-op waker: the sync adapter's futures are ready at once** | **awaits `fetch` and OPFS promises (wasm-bindgen-futures)** | **none (`Waker::noop`, Rust 1.85)** |

The core is still free of I/O: it only awaits the trait's methods, so it is the plan, written
as straight-line code. Chosen: the async core. The native functions keep their signatures and
their tests, which then prove both drivers run one path.

## 6. Impact on the plan

- A new step for TG-25, in phase 8 next to TG-24:
  1. sans-I/O refresh and store plans, the native client moved onto them;
  2. the wasm exports and the worker;
  3. browser tests in CI (Playwright: Chromium, Firefox, WebKit) against a local mirror with
     CORS;
  4. docs: the store page gets a "In the browser" section.
- TG-24's public mirror page names the browser's needs: CORS with `Range` allowed, and
  `Access-Control-Expose-Headers: Content-Range, Content-Length`.
- ADR: one for the browser store (this note's choice), once the prototype has measured it.

## 7. Risks and open items

- **Wasm memory for large packs:** a pack is read into wasm memory. If Nepal's full pack is too
  heavy on phones, read sections lazily through the sync access handle instead. That needs a
  section source in the engine; measure first.
- **Safari OPFS:** there have been bug reports around sync access handles in older Safari
  versions. The CI WebKit run and a real iPhone test decide.
- **Eviction:** stored packs can vanish, especially on Safari. The app must stay correct on the
  baseline alone, and the store reinstalls lazily.
- **The catalog is not live** (key ceremony, TG-24). Until it is, browser tests use a test
  catalog signed with throwaway keys, the way the native tests do.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| MDN browser-compat-data | `main` | 2026-11-17 | github.com/mdn/browser-compat-data (`api/*.json`, `webassembly/jspi.json`) |
| XMLHttpRequest Living Standard | living | 2026-11-17 | xhr.spec.whatwg.org |
| Service Workers (Cache) | editor's draft | 2026-11-17 | w3c.github.io/ServiceWorker |

## 9. Sources

- [MDN browser-compat-data](https://github.com/mdn/browser-compat-data): `api/FileSystemSyncAccessHandle.json`, `api/FileSystemFileHandle.json`, `api/FileSystemHandle.json`, `api/FileSystemDirectoryHandle.json`, `api/StorageManager.json`, `api/LockManager.json`, `api/XMLHttpRequest.json`, `webassembly/jspi.json`, accessed 2026-11-17.
- [XMLHttpRequest Living Standard](https://xhr.spec.whatwg.org/), WHATWG, accessed 2026-11-17.
- [Service Workers](https://w3c.github.io/ServiceWorker/), W3C editor's draft, `Cache.put`, accessed 2026-11-17.
- [Offline loading and trust](offline-loading-and-trust.md) §3.3 (quotas, eviction, OPFS), 2026-11-06.
- [Node.js and WebAssembly packaging](node-wasm-packaging.md) (what the wasm package does today).
