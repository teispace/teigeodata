---
title: "Offline loading and trust: catalog, downloads, storage, updates"
status: research
updated: 2026-11-10
owner: research-agent
revisit: 2027-02-06
---

# Offline loading and trust: catalog, downloads, storage, updates

## 1. Question

How should the optional `sync` capability of
[ADR-0012](../../03-decisions/adr/ADR-0012-offline-loading.md) be built, so that an end user can
pick "Dubai" in an app and the SDK downloads, verifies, stores and loads the right packs, safely
and with little work for the app developer? In detail:

1. **Trust and update security.** Which signed-metadata scheme protects the catalog and packs
   against the known update attacks, how keys rotate, and what a minimal TUF-style scheme keeps.
2. **Delta updates.** Section re-download, binary patches, semantic diffs or content-defined
   chunking, and how BLAKE3 verified streaming fits.
3. **Download managers.** Range requests, ETags, GitHub Releases behaviour, a Rust HTTP client
   versus the platform client, and OS background downloads.
4. **Storage and atomic install.** Rename and fsync rules per OS, crash safety, quota and
   eviction, memory mapping of downloaded and bundled files, repair.
5. **Catalog and developer experience.** How the best offline SDKs expose regions, and our API,
   catalog format and file naming.
6. **Hosted-ready.** How mirrors, CDNs and authenticated mirrors fit the same trust chain
   ([ADR-0025](../../03-decisions/adr/ADR-0025-hosted-ready.md)).

It feeds ADR-0012 (refines it), [ADR-0009](../../03-decisions/adr/ADR-0009-pack-format.md) (the
section table and signature fields), [ADR-0010](../../03-decisions/adr/ADR-0010-addressing-and-baseline.md)
(physical file names next to the logical address), [ADR-0011](../../03-decisions/adr/ADR-0011-layered-engine.md)
(install as a snapshot swap, delta packs as layers), open question Q-006 (hosting), and roadmap
phase 4 (engine, baseline and sync).

Constraints that drive every verdict:

- **Offline-first.** Expired or missing metadata must never break queries over packs already
  installed or bundled. Trust checks gate *installing*, not *reading*.
- The library never touches the network unless the app calls a catalog or download API.
- One Rust core for native, mobile and WASM; bindings for Dart, Kotlin, Swift, TypeScript,
  Python and others ([ADR-0016](../../03-decisions/adr/ADR-0016-bindings.md)).
- Hosting is static files: GitHub Releases first, mirrors optional. No server logic.
- Runtime dependencies stay few and small ([ADR-0009](../../03-decisions/adr/ADR-0009-pack-format.md)):
  `blake3`, `ed25519-dalek` are already in.

## 2. Summary and recommendation

**Trust chain.** Adopt **TUF metadata (spec 1.0.37) with a deliberately small subset**: the four
top-level roles (root, timestamp, snapshot, targets), no delegations, consistent snapshots, and
pack details in each target's `custom` field. Write **our own small verifying client** in the
`sync` crate (a few hundred lines over `ed25519-dalek`, `blake3`, `serde_json`) and use
`tough` 0.24 and python-tuf as **differential test oracles** in CI. Root keys are offline
(threshold 2 of 3), the timestamp key is online in a scheduled CI job. The client persists the
highest versions it has seen, enforces lengths on every fetch, and walks the root chain on
rotation. Packs keep their own Ed25519 signature (ADR-0009) so a bundled or side-loaded pack is
verifiable without any catalog. Sigstore, minisign and signify are not the client trust path:
minisign and signify have no rotation, expiry or rollback protection; Sigstore needs online
transparency-log and identity checks. Sigstore does fit **build provenance** (GitHub immutable
releases produce Sigstore-format attestations).

**Update strategy.** The primary delta is **section-level re-download**: the client fetches the
new pack's header and section table (the first 16 KiB, ADR-0009) with one range request,
compares per-section BLAKE3 hashes with the installed pack, fetches only changed sections by
range, assembles the new file from local and fetched sections, and verifies the whole-file hash.
Repair is the same operation against the pack's own signed hashes. Two candidates must win an
A/B gate before they ship: **zstd `--patch-from` patch assets** for the most common upgrade
(N−1 → N), and **delta packs as layers** (ADR-0011: a small pack of changed records plus
tombstones, merged live, compacted later). Content-defined chunking (casync/desync, zsync) is
rejected for v1: chunk stores do not fit GitHub's 1000-assets-per-release limit, and GitHub's
asset host refuses multi-range requests (measured: HTTP 501).

**Download and storage architecture.** The Rust core owns a **sans-I/O download planner and
installer** (plan, range requests to make, verification, atomic install, journal). Transports are
thin and pluggable: a default pure-Rust client for desktop, server and CLI, and the platform
client on mobile and web (URLSession, OkHttp or Cronet, `fetch`), so OS background downloads,
proxies and the user's trust store work as users expect. Files are written to a staging folder on
the same volume, verified, fsynced, renamed into a **versioned file name** (never over a file
that may be mapped), and committed by an atomically replaced install journal. Then the engine
swaps its snapshot (ADR-0011). Eviction is LRU by last use with pinning and dependency
protection; quota is off unless the developer sets one.

**Hosting.** Small signed metadata on **GitHub Pages** (measured: `access-control-allow-origin: *`,
ETag, `max-age=600`). Packs on **GitHub Releases** with immutable releases. GitHub release assets
send **no CORS headers** (measured), so browser apps need a CORS-enabled mirror. This answers
Q-006: GitHub Releases alone is enough for native apps; the web needs a mirror. Mirrors,
CDNs and a future authenticated hosted tier only *serve bytes*; trust never depends on them.

**Crates.** Use `ed25519-dalek` 3.0, `blake3` 1.8, `serde_json` 1.0 in the client; `ureq` 3.5
(rustls, platform verifier) as the default native transport, behind a feature; `bao-tree` 0.17
as a candidate for in-section range verification; `zstd` 0.14 in the builder for patch assets
under test; `tough` 0.24 only as a dev-dependency oracle. Build ourselves: the TUF subset client,
the planner and installer, the journal, the eviction policy, and the platform transport adapters.

**Unverified** items are marked in place. The biggest ones: iOS mapping limits for very large
files, Play Asset Delivery limits, whether GitHub's 2025 unauthenticated rate limits apply to
release downloads, and the exact HERE and Mapbox estimate APIs.

## 3. Findings

### 3.1 Trust and update security

**TUF.** The specification is at version 1.0.37 (last modified 2026-10-08). Its roles:

| Role | Signs | Key placement |
|---|---|---|
| Root | the keys and thresholds of every top-level role | offline, threshold |
| Targets | which target files are trusted: length, hashes, custom data; may delegate | offline or semi-offline |
| Snapshot | the version of every targets metadata file | online or semi-online |
| Timestamp | a short-lived statement of the snapshot's hash and version; fetched first | online, automated |
| Mirrors (optional) | a list of mirrors and what each serves | rarely used |

Attacks it defends against, in the spec's words: arbitrary installation, **endless data**
(oversized responses), extraneous dependencies, **fast-forward** (inflated version numbers make
later real updates look like rollbacks), **indefinite freeze** (stale metadata served forever),
malicious mirrors, **mix-and-match** (metadata that never existed together), **rollback** (older
files than the client already knows), key compromise below threshold, and wrong software. It does
not prevent denial of service but must *detect* that the client cannot update.

Client workflow (spec §5): fix the start time; load the trusted root shipped out of band; fetch
`N+1.root.json`, `N+2.root.json` … until none exists, each signed by a threshold of the *old*
root's keys **and** a threshold of its *own* keys, version exactly one higher, not expired, and
persisted; if the timestamp or snapshot keys changed, delete the stored timestamp and snapshot
(this is the fast-forward recovery); then timestamp (signature, version not lower, snapshot
version not lower, not expired), snapshot (hash matches the timestamp, version matches, no
targets version decreased, none dropped), targets (hash matches the snapshot, not expired), and
finally the target file, downloaded **up to its expected length** and hash-checked. Consistent
snapshots name metadata `VERSION.name.json` and targets `HASH.name`, so a client mid-update never
sees a mix of old and new files; `timestamp.json` keeps a fixed name.

**What a minimal scheme must keep** (our reading of the attack list against our setting):

| Keep | Why | How in our design |
|---|---|---|
| Offline root with threshold and the N+1 rotation chain | recovers from any non-root key loss; root compromise needs several keys | 2-of-3 Ed25519 root keys on hardware tokens or air-gapped machines; one-year expiry; every `N.root.json` kept forever |
| Timestamp with short expiry | the only defence against freeze on static hosting | 14-day expiry, re-signed daily by a scheduled workflow |
| Persisted highest versions | rollback and fast-forward | store root, timestamp, snapshot, targets versions and each installed pack's release version; refuse lower |
| Length on every fetch | endless data | hard caps for metadata (root 64 KiB, timestamp 16 KiB), exact lengths from signed metadata for everything else |
| Snapshot pinning the targets hash | mix-and-match | one `targets.json` for v1; snapshot kept for spec compatibility and future splits |
| Hash *and* length of every pack in targets | wrong software, arbitrary installation | BLAKE3 of the whole file plus length; the pack's own signature checked as well |
| Dependencies in signed data | extraneous dependencies | `custom.deps` in each target, resolved only through the signed catalog |

**What we drop for v1:** delegations (and hashed bins), the mirrors role, multi-repository
consensus (TAP 4). A world catalog of about 5,000 targets at about 300 bytes each is about
1.5 MB of JSON before compression (**estimate**, to be measured); region names for display come
from the embedded baseline (ADR-0010), so the catalog does not carry them. Delegations return
only if a gate shows the catalog too large.

**Offline-first rule we add.** Expired metadata blocks *installing or updating* and makes
`check_updates()` report `MetadataExpired`; it never blocks reads of installed packs. This keeps
TUF's freeze detection without ever bricking an offline app.

**`tough` (AWS).** Version 0.24.0 (2026-07-10), MIT or Apache-2.0, actively maintained (last
push 2026-11-03). Its docs say it targets TUF 1.0.0 and that "delegated roles (and TAP 3) are
not yet supported". Async (`async-trait`, `tokio`), pulls `aws-lc-rs`, `rustls`, `tokio-util`,
`olpc-cjson` (canonical JSON), `globset`, `walkdir` and others; HTTP behind a feature. It has a
`Transport` trait with filesystem and HTTP transports, `ExpirationEnforcement::{Safe, Unsafe}`
and an editor plus the `tuftool` CLI to build repositories. Too heavy and too tied to tokio and
the filesystem for our WASM and mobile core; a good **oracle and repository-building tool**.

**`tuf` (rust-tuf).** Version 0.2.0 (2025-10-20), MIT or Apache-2.0. Its README says "Beta
Software … may not be suitable for production use … API is unstable". Depends on `ring`,
`hyper`, `chrono`. Not a fit.

**Canonical JSON.** TUF signs the canonical-JSON form of the `signed` object. Our client must
therefore canonicalise before verifying (OLPC canonical JSON as used by `tough` via `olpc-cjson`,
or the spec's guidance). This is the main extra code a TUF-compatible client costs, and it is
small. A differential test against `tough` and python-tuf proves it.

**Sigstore and cosign.** Sigstore's own trust root is a TUF repository kept by five keyholders
from different organisations. Signing identity comes from an OIDC flow embedded in a short-lived
Fulcio certificate, and "Clients MUST NOT trust certificates that are not in the log" (Rekor).
Verification therefore needs the transparency-log evidence and a fresh trust root; offline
verification with a bundle exists but still pins Sigstore's root and identity policy. For an
offline data SDK whose verifier runs on phones and in browsers this is heavy and adds external
trust. It is a good fit for **release provenance**: GitHub immutable releases (generally
available since 2025-10-28) produce signed attestations in Sigstore bundle format.

**minisign and signify.** minisign uses Ed25519 over a BLAKE2b-512 prehash, a key ID of 8
random bytes, and a *trusted comment* covered by a global signature; it can verify signify
signatures but not the reverse. `minisign-verify` 0.3.0 has no dependencies. Neither tool has
key rotation, expiry, versioning or rollback protection, so on its own it cannot stop rollback
or freeze. Fine for a detached signature on a single file, not for a catalog.

**How others secure offline data.**

| Product | Catalog | Integrity | Authenticity | Source |
|---|---|---|---|---|
| Organic Maps | `countries.txt` JSON tree: `id`, `n`, `v` (version), `c`, `s` (size), `h` (hash), `rs`, `g` (children) | `h` = BLAKE3 of the `.mwm`, truncated to 9 bytes (12 base64 chars), replacing legacy `sha1_base64` | the catalog ships inside the app, so the app-store signature covers it (**unverified**: no separate signature found in the generator code) | generator `hierarchy_to_countries.py` |
| OsmAnd | `get_indexes` XML over HTTPS: name, date, size, `containerSize`, `contentSize`, timestamp | none in the index (measured 2026-11-06) | TLS only | `download.osmand.net/get_indexes` |
| Mapbox | TileStore regions and style packs; packs have expiration dates | internal | access token and TLS; terms forbid redistributing offline data | Mapbox offline docs |
| HERE SDK | `MapDownloader` regions, `MapUpdater` catalog updates | internal | credentials and TLS; maps compatible for one year back and forward | HERE offline guide |
| App stores | the OS verifies the app signature; data downloaded later is the app's responsibility | n/a | n/a | n/a |

Lesson: no map SDK we found signs its offline catalog end to end; they rely on TLS to their own
servers. We serve from third-party hosts and mirrors, so end-to-end signatures are required, not
optional. Organic Maps' move to BLAKE3 is a sign of where the field is going, but a 72-bit
truncated hash is integrity, not security; we keep full 256-bit hashes.

### 3.2 Delta updates

**Section-level re-download.** A `.tgd` pack is sectioned with a BLAKE3 hash per section
(ADR-0009), and builds are reproducible. When a release changes only some columns or some
regions, most sections keep the same bytes and the same hash. The client needs only the new
section table (one range request for the first 16 KiB) and then single-range requests for the
changed sections. No extra assets are needed on the host. Its saving depends on how stable
section boundaries are between releases, which the builder can favour (sort order fixed by
stable ID, per-section codecs chosen deterministically). **Gate needed:** measure the fraction of
bytes reused between two real monthly builds.

**Binary diffs.**

| Tool | Published result | Notes |
|---|---|---|
| bsdiff | Chrome dev-channel update of about 10.4 MB: bsdiff patch 704,512 bytes (Chromium blog, 2009-07-15) | slow, memory hungry; `qbsdiff` 1.4.4 is a compatible Rust port (bzip2, rayon) |
| Courgette | same update: 78,848 bytes, about 9× smaller than bsdiff, by disassembling x86 and diffing symbolic labels | specific to executables; Chrome later moved to Zucchini (**unverified** date) |
| xdelta3 / VCDIFF (RFC 3284) | zstd wiki: patch size "slightly worse than zstd"; fastest at creating text patches | Rust crates are stale (`xdelta3` 0.1.5 of 2019, `vcdiff-decoder` 1.0.0 of 2024) |
| zstd `--patch-from` | zstd wiki (2020): at level 19 patch size "comparable with bsdiff", better on large inputs; level 1 over 200× and level 3 over 100× faster than bsdiff at creating; fastest to apply; reference up to 2 GB | applying needs the zstd decoder with a dictionary of the old file; `ruzstd` decoding with a raw-content dictionary of that size is **unverified** |

Binary diffs over *compressed* columnar sections are usually poor, because a one-record change
shifts every byte of the compressed block after it. They work best on the uncompressed or lightly
encoded form. Since our sections are already the unit of change, a patch per changed section
(old section → new section) is the natural shape if patches win at all.

**Semantic record diffs as layers.** ADR-0011 already merges layers by stable ID with
tombstones. A "delta pack" with only changed or deleted records is a semantic diff that needs no
new format and no patch code: install it as a layer, and compact later. It costs query-time merge
work and grows with the number of deltas, so a policy is needed (for example, after three deltas,
fetch the full section set). **Gate needed:** delta-pack size versus section re-download on two
real releases, and query latency with 0, 1 and 3 layers.

**Content-defined chunking.**

- FastCDC (Xia et al., USENIX ATC 2016; IEEE TPDS 2020): Gear rolling hash with normalised chunk
  sizes; about 10× faster than Rabin-based CDC and about 3× faster than other Gear or AE
  chunkers, with similar deduplication. `fastcdc` 5.0.0 is pure Rust and dependency-free by
  default.
- casync and desync: chunk stores of content-addressed files plus `.caibx` index files; desync
  supports HTTP, S3/GCS, SFTP and OCI stores, zstd compression, SHA512/256 or SHA-256 chunk
  hashes, and *seeds* (reuse of local files). casync's default chunk sizes of 16/64/256 KiB are
  **unverified** from a primary source here.
- zsync: the client computes rolling checksums over its old file and fetches missing blocks by
  range from one big file, using a `.zsync` control file. Efficient with multi-range requests.

Two facts rule CDC out for v1 on GitHub: a release holds at most **1000 assets** (GitHub docs),
so a chunk store of tens of thousands of chunks does not fit; and the asset host answers a
multi-range request with **HTTP 501** (measured 2026-11-06), so zsync degrades to one request per
block. Section re-download gets most of zsync's benefit with requests the size of sections.

**BLAKE3 and Bao verified streaming.** BLAKE3 is a Merkle tree over 1 KiB chunks, so any range
can be verified against the root hash given the sibling hashes on its path. Bao's spec: "the
decoder verifies the chaining value of each node as it's read … any output byte returned by the
decoder matches the corresponding input byte". An outboard tree costs one 64-byte parent node per
pair of leaves: about 64 bytes per 1 KiB chunk (6.25 %) with plain Bao, and about 64 bytes per
16 KiB group (about 0.4 %) with `bao-tree`'s chunk groups (**derived** from the tree structure,
not a published measurement). `bao-tree` 0.17.0 (2026-10-22, by n0, used by `iroh-blobs`)
supports chunk groups (16 KiB is its suggested default), `ChunkRanges` for arbitrary range sets,
sync and async I/O, and is compatible with `bao` only at block size 1024 and a single range.
`abao` 0.2.0 is from 2023 and unmaintained; `bao` 0.13.1 is the reference implementation.
The value for us: the catalog carries one hash (BLAKE3 of the file); an outboard served next to
the pack lets the client verify each received block **before writing it**, so a bad mirror is
detected within one block, resumes need no re-hash, and two mirrors can serve halves of a file.
Without an outboard, resume works by re-hashing the downloaded prefix locally, which BLAKE3 does
at memory speed (the BLAKE3 README shows it faster than MD5, SHA-1, SHA-2, SHA-3 and BLAKE2; we
did not run a benchmark, per the no-build rule).

### 3.3 Download managers

**GitHub Releases, measured on 2026-11-06** (public asset of `BurntSushi/ripgrep` 14.1.1):

| Behaviour | Result |
|---|---|
| `github.com/…/releases/download/…` | HTTP/2 `302` to `release-assets.githubusercontent.com` (not `objects.githubusercontent.com` as older docs and tools assume), `cache-control: no-cache` |
| Redirect target | Azure Blob behind Varnish (Fastly-style `x-served-by`, `x-cache: HIT`); a signed URL with an expiry about **1 hour** after issue (`se=` minus `skt=`) |
| Single range | `206`, `content-range: bytes 100-199/2566310`, `accept-ranges: bytes` |
| Multi-range (`bytes=0-9,100-109`) | `501` |
| ETag | strong-looking `"0x8DCD…"`; `If-None-Match` gives `304` |
| `If-Range` with a wrong ETag | still `206` (should be `200` per RFC 9110) — **the host ignores `If-Range`**, so a resumed download can silently mix two versions unless the client checks ETag, length and hashes itself |
| CORS | no `access-control-allow-origin` on either hop; preflight `OPTIONS` answers `405` — **browsers cannot fetch release assets directly** |
| HTTP/3 | no `alt-svc` header on either host — HTTP/2 only |
| `releases/latest/download/…` | redirects to the newest tag — never use it for pinned packs |
| API rate limit | unauthenticated REST: 60 requests per hour (`x-ratelimit-limit: 60`); our IP was at 0 remaining after a few calls. Release downloads through `github.com` did not count against it in our probe; GitHub's 2025-05-08 changelog tightened unauthenticated limits, and whether release downloads are covered is **unverified** |
| Limits (GitHub docs) | each file under 2 GiB; up to 1000 assets per release; no limit on total release size or bandwidth |
| GitHub Pages (measured) | `access-control-allow-origin: *`, ETag, `cache-control: max-age=600` — suitable for small signed metadata; Pages' own size and bandwidth limits are **unverified** here |

Consequences: the client must follow redirects, must not cache the signed redirect target past
about an hour, must treat the ETag plus total length (from `Content-Range`) as the identity of a
partial file and check them on every resumed request, must verify hashes regardless, and must
split multi-section fetches into separate single-range requests (cheap on one HTTP/2 connection).
Because packs live at versioned, immutable file names, ETag mismatches should only happen on a
broken or malicious mirror.

**HTTP/2 versus HTTP/3.** GitHub's asset host offers only HTTP/2. HTTP/3 (QUIC) helps on lossy
mobile links and with connection migration, but its Rust stack (`quinn` 0.11, `h3` 0.0.8) is
young and adds size. Platform clients (URLSession, Cronet) negotiate HTTP/3 themselves where a
server offers it. Not worth building into the Rust default.

**Rust HTTP clients.**

| Client | Model | TLS default | Notes |
|---|---|---|---|
| `reqwest` 0.13.5 | async on `tokio` + `hyper` 1.12 | since 0.13.0, rustls by default with the **aws-lc** crypto provider and `rustls-platform-verifier` for roots | HTTP/2, experimental HTTP/3; aws-lc-rs needs a C toolchain (CMake) for iOS, Android and WASM cross-builds; `ring` selectable |
| `ureq` 3.5.1 | blocking, sans-I/O core (`ureq-proto`) | rustls optional, with `rustls-platform-verifier` or `webpki-roots` | HTTP/1.1 only; small dependency tree; fits a worker-thread download loop |
| `hyper` 1.12 directly | async building block | bring your own | only if we need HTTP/2 without reqwest |

Binary size and build cost of each are **not measured** here (no builds). A gate must measure the
release-size delta of the `sync` feature with each client on Linux, Android and iOS.

**Platform clients.** URLSession (iOS, macOS), OkHttp or Cronet (Android), `fetch` (web) already
handle proxies, the user's and the enterprise's trust store, HTTP/2 or HTTP/3, cellular policy,
and — the decisive point — **OS background downloads**. Our core should not own sockets on these
platforms. It plans, verifies and installs; the platform moves bytes.

**iOS background URLSession** (Apple docs, fetched 2026-11-06):

- Only upload and download tasks; transfers run in a separate system process and continue while
  the app is suspended or terminated by the system.
- Recreate the session with the same identifier at launch; implement
  `application(_:handleEventsForBackgroundURLSession:completionHandler:)`, store the handler,
  call it from `urlSessionDidFinishEvents(forBackgroundURLSession:)` on the main queue.
- The downloaded file is available only "until your delegate method returns" — move it at once.
- "If the user terminates the app from the multitasking screen, the system cancels all of the
  session's background transfers … the system does not automatically relaunch apps that were
  force quit by the user."
- `isDiscretionary` lets the system wait for Wi-Fi and power; for transfers started while the app
  is in the background the system always treats them as discretionary.
- Redirects are always followed (the GitHub 302 is fine); only HTTP and HTTPS.
- A rate limiter delays new tasks started from the background, and "the delay increases each
  time the system resumes or relaunches your app".
- Set `countOfBytesClientExpectsToReceive` as a best-guess upper bound (we have exact sizes from
  the catalog).

Design consequence: on iOS, the Rust planner hands URLSession **whole-file or whole-section
download tasks** to files in staging, then "adopts" each finished file for verification and
install. Fine-grained range scheduling happens only in the foreground.

**Android** (Android docs, fetched 2026-11-06):

- **User-initiated data transfer (UIDT) jobs**, Android 14 (API 34)+: `JobInfo.Builder.setUserInitiated(true)`,
  permission `RUN_USER_INITIATED_JOBS`, must be scheduled while the app is visible (or under the
  background-activity-start exceptions), must post a notification via `setNotification()`,
  `setEstimatedNetworkBytes()` recommended, constraints such as unmetered network, charging or
  storage-not-low allowed. Not subject to App Standby quotas; stopped for thermal state, low
  memory, unmet constraints or when it runs "longer than necessary". If the user presses Stop in
  Task Manager the process is killed **without** `onStopJob()`, so transfer state must be
  persisted continuously. No Jetpack library supports UIDT; below Android 14 use a WorkManager
  long-running worker with a foreground service.
- Google Maps moved offline downloads from a foreground service to UIDT and reported "a 10%+
  improvement in download failure rate" (Android Developers Blog, 2024-09-03).
- **Android 15** limits `dataSync` foreground services to **6 hours in 24** for apps targeting
  API 35, then calls `Service.onTimeout()`; the timer resets when the user brings the app to the
  foreground.

**Web.** Background Fetch is experimental and ships only in Chromium (Chrome 74+; not Firefox or
Safari, per MDN browser-compat-data 2026-11-06); it needs a service worker and shows browser UI.
For everything else, downloads run while the page or a service worker is alive, with
`fetch` + `Range` and progress from the response stream. And the GitHub CORS finding above means
web apps must use a CORS-enabled mirror or their own origin.

**Browser storage** (MDN, fetched 2026-11-06):

- Quotas are per origin and cover IndexedDB, Cache API and OPFS together.
- Chromium: up to 60 % of total disk per origin. Firefox: best-effort the smaller of 10 % of disk
  or 10 GiB per site group; persistent up to 50 % of disk (cap 8 TiB). Safari: about 60 % of disk
  per origin in the browser, about 15 % in WebViews of other apps.
- Eviction under pressure is LRU by origin, whole origin at once, and never touches persistent
  origins. **Safari deletes all script-written storage of an origin with no user interaction in
  the last seven days of browser use** (with tracking prevention on); Home Screen web apps use the
  browser quota.
- `navigator.storage.persist()`: Firefox prompts; Chromium and Safari decide silently from
  engagement. Available since Chrome 55, Firefox 57, Safari 15.2; `estimate()` since Safari 17.
- OPFS `getDirectory()`: Chrome 86, Firefox 111, Safari 15.2; `FileSystemSyncAccessHandle`
  (synchronous positional reads and writes in a worker): Chrome 102, Firefox 111, Safari 15.2.
  This is the right store for packs in WASM: read sections at offsets without loading the whole
  file into memory.

### 3.4 Storage and atomic install

**Rename.** Rust's `std::fs::rename` is `rename(2)` on Unix and `MoveFileExW` with a fallback
to `SetFileInformationByHandle` on Windows; on Windows 10 1607+ with `FileRenameInfoEx` it has
Unix semantics. It fails across mount points, so staging must be on the same volume as the
install folder. On Windows, replacing or deleting a file that is open or memory-mapped without
share-delete fails (well-known Windows behaviour; **unverified** here against current docs) — one
more reason to install each version under a **new file name** and delete old versions only when
no snapshot maps them.

**fsync.** Linux `fsync(2)`: "Calling fsync() does not necessarily ensure that the entry in the
directory containing the file has also reached disk" — fsync the directory too. On Apple
platforms plain `fsync` does not flush the drive cache; Rust's `File::sync_all` and
`sync_data` already call `fcntl(F_FULLFSYNC)` there (checked in `library/std/src/sys/fs/unix.rs`,
2026-11-06). On Windows, `sync_all` is `FlushFileBuffers`; directories cannot be fsynced the same
way, and the rename is made durable by the file system journal (**unverified** detail). The
install order is therefore: write to staging → `sync_all` the file → rename into place → fsync
the directory (Unix) → write the new journal to a temp file → `sync_all` → rename over the old
journal → fsync the directory. The journal rename is the commit point.

**Crash safety.** On start, recovery reads the journal; files in `packs/` that the journal does
not list are leftovers of an interrupted install and are deleted; files in `staging/` with a valid
resume record are kept for resume; packs the journal lists but that fail a cheap check (header,
section table hash, length) are marked for repair. A full verification is lazy or on demand.

**iOS.**

- Store packs in Application Support (Caches can be purged by the system) and set
  `isExcludedFromBackup` on each file, because packs are re-downloadable; Apple notes the flag
  must be set "each time you save a file because some common file operations cause this property
  to reset".
- File protection: the default is "complete until first user authentication", readable after
  the first unlock even when locked. `complete` makes files unreadable while locked, which breaks
  background launches and any mapped reads after lock. Packs are public data: keep the default.
- Free space: `volumeAvailableCapacityForImportantUsageKey` is a **required-reason API**: an SDK
  using it must declare it in its `PrivacyInfo.xcprivacy`. Our Swift package must ship that
  manifest.
- Mapping very large files can fail on iOS because of the per-process virtual address limit
  (**unverified**; to test on a device). Map per section or per pack, not one huge mapping.

**Android.**

- App-specific internal storage (`filesDir` or `noBackupFilesDir`) needs no permission and is
  not visible to other apps; it suits packs. `StorageManager.getAllocatableBytes()` gives the
  space the app may allocate (counting cache the system could clear), and `ACTION_MANAGE_STORAGE`
  with `EXTRA_REQUESTED_BYTES` asks the user to free space.
- Packs bundled in the APK or AAB must be stored **uncompressed** so `AssetManager.openFd()` can
  map them: "The asset must be uncompressed, or an exception will be thrown". In Gradle:
  `androidResources { noCompress += "tgd" }`. An uncompressed entry starts at an offset aligned
  only to what zipalign gives (4 bytes for general entries; **unverified** whether newer AGP aligns
  assets to 16 KiB), so the reader must not assume the pack starts on a page boundary. ADR-0009's
  unaligned `zerocopy` reads already cope; mapping must use the `AssetFileDescriptor` offset.
- Very large bundled data can use Play Asset Delivery install-time packs (limits **unverified**).

**Quota and eviction.** Mapbox's TileStore is a useful reference: no quota by default;
`TileStoreOptions.diskQuota` when set; eviction starts at 50 % of the quota (since SDK 11.2.0)
and removes the packs with the nearest expiry that no region shares; it refuses new data that
would exceed the quota. HERE separates an LRU *cache* from *persistent* downloaded regions that
are never evicted automatically. Our policy: bundled packs never count and are never evicted;
packs the user or developer pinned are never evicted; a pack another installed pack depends on is
protected while the dependent stays; everything else is LRU by last query time (recorded coarsely,
for example per day, to avoid write amplification). Eviction runs before an install to make room,
and an install that still does not fit fails with the exact numbers.

**Repair.** `verify(deep)` re-hashes sections against the signed table; a bad section is
re-fetched by range, which is the same code path as a section-level delta. A pack whose signature
or header is bad is re-downloaded whole.

### 3.5 Catalog and developer experience

**API shapes of the best SDKs.**

- **Mapbox** (Maps SDK v11): `OfflineManager.loadStylePack(for:loadOptions:progress:completion:)`;
  `TileStore.default.loadTileRegion(forId:loadOptions:progress:completion:)` returning a
  `Cancelable`; calling it again with the same ID *updates* the region; `allTileRegions`,
  `removeTileRegion` (removal is deferred to cleanup); per-region `metadata`; `acceptExpired`;
  progress `TileRegionLoadProgress` with `requiredResourceCount`, `completedResourceCount`,
  `loadedResourceCount`, `erroredResourceCount` and byte sizes; `TileRegionEstimateProgress` for
  estimates (the exact `estimateTileRegion` signature is **unverified**). Callbacks are not on the
  main thread; cancellation is a distinct error (`TileRegionError.canceled`). Limits and pitfalls:
  at most 750 unique tile packs, no automatic updates, style changes can invalidate regions,
  bounding boxes grow exponentially with zoom, and offline data "may not be preloaded, bundled or
  otherwise redistributed".
- **HERE SDK**: `MapDownloader.downloadRegions()`, `MapDownloader.deleteRegions()`,
  `MapUpdater.updateCatalog()`; regions from continent down to city or a polygon or route
  corridor; `setOfflineMode(true)` blocks all online requests; offline maps are compatible one
  year backward and forward; pre-installing maps needs a contract. Region fields such as
  `sizeOnDiskInBytes` and `sizeOnNetworkInBytes`, the `DownloadRegionsStatusListener` callbacks
  (progress, pause, resume, complete) and a `repairPersistentMap` call are **unverified** here.
- **Google Maps**: offline areas exist in the consumer app (moved to UIDT on Android 14, see
  above); we found no offline-region API for third-party developers in the Maps SDKs
  (**unverified** absence).
- **Organic Maps**: the tree in `countries.txt` with size, version and hash per leaf; the app
  downloads `.mwm` files from its own servers (two servers with 400 TB/month donated bandwidth,
  per its README as quoted in search results).
- **OsmAnd**: a flat XML index with packed and unpacked sizes and timestamps per file, plus
  incremental "Live" updates (details **unverified**).

Pitfalls they show: sizes are estimates that users compare with the real download (report
*download bytes* and *disk bytes* separately, as OsmAnd's `containerSize` and `contentSize` do);
removal that frees space "later" confuses users; no automatic updates means stale data; region
identity that depends on style or zoom choices breaks on change. Our deterministic addresses
(ADR-0010) avoid the last one.

**Our API (proposal).** Rust, in the `sync` crate:

```rust
let sync = Sync::builder()
    .trusted_root(include_bytes!("root.json"))   // shipped root, version N
    .storage(StorageDir::app_default()?)          // or a custom Storage impl
    .mirrors(Mirrors::default_github())           // ordered, pluggable
    .transport(DefaultTransport::new())           // or a platform transport
    .quota(Quota::max_bytes(2 << 30))             // optional
    .build()?;

let catalog = sync.refresh_catalog().await?;           // network; verifies the chain
let plan = sync.plan(Request::region("ae-du").components(&[Places, Admin]))?;
// plan: packs (with dependencies), download_bytes, disk_bytes, reuse_bytes, needs_eviction
let job = sync.start(plan)?;                           // queued; returns a handle
job.pause(); job.resume(); job.cancel();
while let Some(ev) = job.events().next().await { /* Queued, Started, Progress{done,total,rate},
    Verifying, Installing, Installed, Paused(reason), Failed(error) */ }
let updates = sync.check_updates().await?;             // Vec<Update{address, from, to, bytes}>
sync.installed(); sync.pin("np"); sync.remove("ae-du"); sync.verify(Depth::Deep)?;
```

Errors are one enum with a stable code and a `retryable()` flag: `Network`, `NotFound`,
`Integrity { section }`, `Signature`, `Rollback`, `MetadataExpired`, `InsufficientStorage
{ needed, available }`, `Incompatible { min_reader }`, `Cancelled`, `Busy`. The engine picks up
installs automatically through its snapshot swap (ADR-0011); no reload call.

Mapping: Dart `Stream<SyncEvent>` and `Future`; Kotlin `Flow<SyncEvent>` and `suspend` functions,
with a `WorkManager`/UIDT adapter module; Swift `AsyncSequence` and `async throws`, with a
background-URLSession adapter; TypeScript `AsyncIterable` plus `EventTarget`, and `Promise`s,
with an OPFS storage and a `fetch` transport. The job state lives in the journal, so a job survives
process death and a new process can list and resume it.

**Catalog format.** TUF JSON metadata for the signed chain, with pack details in `custom`:

```json
"np.places.tgd": {
  "length": 18432011,
  "hashes": { "blake3": "…" },
  "custom": { "address": "np", "component": "places", "release": "2026.11.0",
              "file": "np.places.2026.11.0.<hash8>.tgd", "disk_bytes": 18432011,
              "deps": ["np.admin.tgd"], "min_reader": 1, "sections": "…hash of section table…" } }
```

TUF requires `sha256`/`sha512` only by convention; the spec allows any hash name. **Measured
2026-11-10 (D-162):** python-tuf 7.0.1 refuses a target that lists an algorithm it does not know
(`Unsupported algorithm 'blake3'`), so targets carry `sha256` alone. A binary catalog
(for example `postcard`) would be smaller and faster to parse but loses interoperability with
existing TUF tools, readability and easy debugging; JSON compresses well over HTTP and is parsed
once per refresh. Recommendation: JSON on the wire; the client may cache a parsed binary form
locally.

**Deterministic addressing.** The logical address stays as in ADR-0010 (`ae-du.places.tgd`).
The physical file adds release and hash prefix, as TUF consistent snapshots do:
`ae-du.places.2026.11.0.<first 8 hex of BLAKE3>.tgd`. Files are immutable, CDN-cacheable forever,
and never overwritten in place on disk.

### 3.6 Hosted-ready

- **Mirrors.** Every mirror serves the same files at the same relative paths. The client tries
  mirrors in order, falls back on network errors, and *demotes* a mirror that served bad bytes.
  Because every byte is checked against signed hashes, a mirror can deny service but cannot change
  data, which is TUF's "malicious mirrors" property.
- **Layout.** `metadata/` on GitHub Pages (CORS, short cache) holding `N.root.json`,
  `timestamp.json`, `N.snapshot.json`, `N.targets.json`; packs on GitHub Releases, one release per
  data release and shard (for example per continent) so no release exceeds 1000 assets, with
  immutable releases on.
- **CDN caching.** Immutable pack names allow `max-age` of a year; `timestamp.json` gets a short
  `max-age` (Pages gives 600 s). Versioned metadata names make stale-cache mix-ups impossible.
- **Authenticated mirrors (future paid tier).** The `Transport` gets an optional credential
  provider that adds headers (token or signed URL) per mirror. Credentials are never part of
  signed data and never change what is trusted; a hosted tier serves the same signed bytes, which
  keeps ADR-0025's "a deployment, not a fork". A tier may add its own targets (private packs)
  only through a key the root delegates to it; that needs delegations, so it is a later step.

### 3.7 The client as built (2026-11-10)

`teigeodata-sync::trust` (feature `trust`, `no_std` + `alloc`, sans-I/O) follows python-tuf's
`TrustedMetadataSet`: the caller hands it bytes in order (root chain, timestamp, snapshot,
targets) and it keeps only what verified. What the build settled:

- **Canonical JSON** is written as `securesystemslib` writes it: keys sorted by UTF-8 bytes, no
  whitespace, only `\` and `"` escaped (a newline or `é` stays raw), integers only. Unknown
  fields stay in the signed bytes, so a changed unknown field breaks the signature.
- **Signatures** are Ed25519 `verify_strict` and counted by distinct public key, not by key ID:
  one key listed under two IDs counts once (spec 5.3.4).
- **Hashes:** `sha256`, `sha512` and `blake3`; an unknown algorithm is skipped, and a target
  needs at least one known one.
- **Length caps** before parsing: root 512,000 B (python-tuf's default; the 64 KiB in §3.1 would
  refuse a root with many keys), timestamp 16,384 B, snapshot 2,000,000 B, targets 5,000,000 B
  or the length the parent signed; at most 256 root rotations in one walk.
- **Offline-first expiry:** expired metadata that verified is still kept and the call returns
  `Expired`, so an app can show what it has; updating stops there.
- **Stored copies:** a stored snapshot skips the length and hash the current timestamp names
  (a newer timestamp may name another), is kept, and returns `Version` when it is not current;
  no targets load over it.
- **Key rotation:** when a new root changes the timestamp or snapshot keys, the stored timestamp
  and snapshot are dropped (fast-forward recovery).
- **Delegations** are refused: the catalog has none (D-159).

Tests: 17 in `trust::tests` on a generated repository (rotation, expiry, rollback of each role,
mix-and-match, freeze, a duplicate key, endless data, malformed metadata, call order). Each of
nine checks (distinct keys, root version step, root expiry, timestamp and snapshot rollback,
targets version in the snapshot, current snapshot, snapshot and targets version) was removed
in turn and a test failed.

**Differential against python-tuf (D-161).** 27 repositories built and judged by python-tuf
7.0.1, 66 steps, replayed by `crates/teigeodata-sync/tests/tuf_differential.rs`. Every verdict
agrees but one: python-tuf counts a threshold by key IDs, so one key listed under two IDs meets
a threshold of 2; we count distinct public keys and refuse it, so one key can never stand for
two signers. python-tuf's own serialization verifies under our canonical JSON,
including control characters, non-ASCII keys and `u64::MAX`. `tough` is still to be added as a
second oracle.

## 4. Options compared

**Trust scheme.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| TUF subset, own client | defends rollback, freeze, mix-and-match, endless data; standard metadata; works offline; small | canonical JSON and the chain logic to write and test | n/a (ours) | spec 1.0.37 (2026-10-08) | **Use** |
| `tough` as the runtime client | complete, maintained, AWS-backed | tokio/async, aws-lc-rs, filesystem datastore; no delegations; heavy for WASM and mobile | MIT OR Apache-2.0 | 0.24.0 (2026-07-10) | Oracle and repo tooling only |
| `tuf` (rust-tuf) | TUF client | self-declared beta, unstable API, `ring` + `hyper` | MIT OR Apache-2.0 | 0.2.0 (2025-10-20) | Reject |
| Sigstore / cosign | identity-based, transparency log | online trust root and log evidence; external trust; heavy verifier | Apache-2.0 | `sigstore` 0.15.0 (2026-11-05) | Provenance only |
| minisign / signify | tiny; `minisign-verify` has no dependencies | no rotation, expiry, versions; no rollback or freeze defence | MIT (`minisign-verify`) / ISC (signify) | `minisign-verify` 0.3.0 | Reject for catalog |
| Pack signature only (ADR-0009) | works without catalog | no freshness, no rollback defence | n/a | n/a | Keep in addition |

**Delta strategy.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Section re-download by range | no extra assets; same path as repair; uses existing hashes | saving depends on stable sections | n/a | n/a | **Primary** |
| Delta packs as layers | no new format; semantic; small for sparse changes | query merge cost; needs a compaction policy | n/a | n/a | Candidate, A/B gate |
| zstd `--patch-from` per section | small patches, fast apply | extra assets per version pair; decoder dictionary size | BSD-3 or GPL-2 (zstd); BSD-3-Clause (`zstd` crate) | `zstd` 0.14.3 | Candidate, A/B gate |
| bsdiff (`qbsdiff`) | well known | slow, memory hungry | BSD-2 (bsdiff); MIT (`qbsdiff`) | 1.4.4 | Reject |
| xdelta3 / VCDIFF | standard format | stale Rust crates; C library | Apache-2.0 | `xdelta3` 0.1.5 (2019) | Reject |
| Courgette / Zucchini | best for executables | executables only | BSD-3 | Chromium | Not applicable |
| CDC chunk store (casync/desync, FastCDC) | dedup across versions and regions | 1000-asset limit; many requests | LGPL-2.1 (casync, **unverified**), BSD-3-Clause (desync), MIT (`fastcdc`) | `fastcdc` 5.0.0 | Reject for v1 |
| zsync | single file + control file | needs multi-range (GitHub: 501) | Artistic-2.0 (zsync, **unverified**); MIT (`zsync-rs`) | `zsync-rs` 0.3.0 | Reject |

**In-section verification.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Re-hash prefix on resume | zero format cost | verification only at section end; no multi-mirror split | n/a | n/a | **Default** |
| `bao-tree` outboard sidecar | verify every 16 KiB group before writing; any range; root = file BLAKE3 | extra asset per pack (about 0.4 %); dependency (`positioned-io`, `range-collections`, `self_cell`, `smallvec`) | MIT OR Apache-2.0 | 0.17.0 (2026-10-22) | Candidate, gate |
| Our own block-hash list in the manifest | trivial code | duplicates what BLAKE3's tree gives | n/a | n/a | Fallback |

**Transport.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `ureq` 3 + rustls + platform verifier | small, blocking, sans-I/O core | HTTP/1.1 only | MIT OR Apache-2.0 | 3.5.1 (2026-11-02) | **Default native** (size gate) |
| `reqwest` 0.13 | HTTP/2, async, ubiquitous | tokio; aws-lc-rs by default needs C toolchain | MIT OR Apache-2.0 | 0.13.5 | Alternative if HTTP/2 measured to matter |
| Platform clients (URLSession, OkHttp/Cronet, fetch) | OS background downloads, user trust store, HTTP/3, proxies | per-platform adapter code | platform | n/a | **Default on iOS, Android, web** |

**Catalog encoding.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| TUF JSON | interoperable, tools exist, readable | larger; canonical JSON for signatures | n/a | spec 1.0.37 | **Use** |
| Own binary (postcard) | small, fast | no tools; own signing rules | MIT OR Apache-2.0 | `postcard` 1.1.3 | Local cache form only |

## 5. What we build ourselves, and why

1. **TUF-subset verifying client** (`sync::trust`). No crate fits a `no_std`-leaning, WASM- and
   mobile-friendly core. Scope: root chain walk, timestamp/snapshot/targets checks, canonical
   JSON, threshold Ed25519, persisted versions, length caps, offline-first expiry rule.
   Proof: differential tests against `tough` and python-tuf on generated repositories, plus the
   TUF attack suite below.
2. **Sans-I/O planner and installer** (`sync::plan`, `sync::install`). Computes what to fetch
   (whole file, sections, patches), consumes bytes from any transport, verifies, writes staging,
   commits through the journal. Proof: crash-injection and resume tests.
3. **Journal and recovery** (`sync::journal`). A small versioned file replaced atomically.
4. **Storage manager**: quota, LRU with pinning and dependency protection, repair.
5. **Platform adapters** in the bindings: iOS background URLSession, Android UIDT with WorkManager
   fallback, web `fetch` + OPFS (and Background Fetch where present).
6. **Repository tooling** in `xtask` or the builder: sign, publish metadata to Pages, upload packs
   to sharded releases, refresh `timestamp.json` on a schedule. `tuftool` can help early on.

## 6. Impact on the plan

- **ADR-0012** (refine): TUF 1.0.x subset with our own client; offline-first expiry rule;
  sans-I/O core with platform transports; versioned file names; journal as commit point;
  section re-download as the primary delta.
- **New ADR** (proposed): "Signing keys and metadata hosting": 2-of-3 offline root, roles and
  expiries, metadata on GitHub Pages, packs on sharded immutable releases, CORS mirror for web.
- **ADR-0009**: the section table must be in the first 16 KiB and hashable on its own (its hash
  goes into `custom.sections`); builds must keep section boundaries stable between releases where
  data did not change.
- **ADR-0010**: add the physical name `address.component.release.hash8.tgd`.
- **ADR-0011**: delta packs are layers; add a compaction policy if the A/B gate keeps them.
- **ADR-0025**: credential provider on the transport; authenticated mirrors serve the same bytes.
- **Q-006**: answer "GitHub Releases for native; a CORS mirror is required for web" — record in
  the decision log when the maintainer agrees.
- **Roadmap phase 4**: add the gates below; add a "Swift privacy manifest" item and an Android
  `noCompress` note to the binding tasks.
- **versions.md**: add the crates in §8.

**Gates** (each must be shown to fail on a deliberate violation first):

1. **Rollback:** serve an older timestamp, snapshot, targets or pack release → `Rollback`, nothing
   installed.
2. **Fast-forward recovery:** serve a timestamp with an inflated version, then rotate the
   timestamp key in a new root → client recovers.
3. **Freeze:** serve an expired timestamp → `MetadataExpired` on update; queries on installed
   packs still answer.
4. **Mix-and-match:** targets whose hash does not match the snapshot → rejected.
5. **Endless data:** a mirror that streams forever → aborted at the signed length (or cap) plus
   zero bytes.
6. **Root rotation:** client with root v1 walks to v3; a v2 signed by only the new keys is
   rejected.
7. **Differential:** our client and `tough` agree on accept/reject for a corpus of generated
   repositories, including the attack cases.
8. **Crash in the middle of install:** kill the process (SIGKILL, and power-cut simulation with a
   fault-injecting storage) at every write, fsync and rename step → after restart, either the old
   or the new version is installed, never a mix, and no orphan files remain.
9. **Resume:** interrupt at random byte offsets, resume against the same and a changed ETag; a
   mirror that ignores `If-Range` (as GitHub does) must never produce a mixed file.
10. **Bad mirror:** a mirror flips one byte → detected, mirror demoted, next mirror used.
11. **Section delta:** two real monthly builds → bytes downloaded with section re-download, with
    patches, with delta packs; the winner becomes the default.
12. **Catalog size:** compressed targets metadata for the world ≤ 512 KiB, parse ≤ 20 ms on a
    mid-range phone (budgets to confirm).
13. **Binary size:** `sync` feature size per platform with `ureq` and with `reqwest`.
14. **Platform:** iOS background download completes after the app is suspended and relaunched by
    the system; Android UIDT job survives `adb shell cmd jobscheduler timeout`; web OPFS install
    survives a reload.
15. **Eviction:** pinned, bundled and depended-on packs are never evicted; quota respected.

## 7. Risks and open items

- **Timestamp key online in CI.** A compromise allows freeze or rollback within the timestamp
  expiry window, never forged data. Mitigation: short expiry, separate key, rotation through root.
- **Signing on static hosting needs a daily job.** If the scheduled workflow stops (GitHub
  disables schedules on inactive repositories; **unverified** for this repo's activity level),
  every client starts reporting `MetadataExpired` after 14 days. Monitor it.
- **GitHub behaviour changes.** The redirect host already changed from
  `objects.githubusercontent.com` to `release-assets.githubusercontent.com`; rate limits tightened
  in 2025. Keep mirrors pluggable and test against the live host in a scheduled job.
- **`If-Range` ignored by GitHub's asset host** (measured). Our resume logic must not depend on it.
- **Web needs a CORS mirror** — a cost or a dependency on a free tier (for example R2 or another
  bucket); choose before the web binding ships.
- **Safari's seven-day storage deletion** can erase downloaded packs of a web app that users open
  rarely. Document it; recommend installed web apps; detect and re-download.
- **iOS force quit cancels background transfers.** The journal must make the next launch resume.
- **Canonical JSON corner cases** (number formats, Unicode escaping) are a classic source of
  verification mismatches; the differential gate covers them.
- **aws-lc-rs build requirements** if `reqwest` is chosen; prefer the `ring` provider or `ureq`.
- Open: shard scheme for releases (per continent, per component, or per release month); whether
  delegations are needed for a hosted tier's private packs; the exact HERE and Mapbox estimate
  APIs (only for comparison).

## 8. Versions checked

All crate data from the crates.io API (`https://crates.io/api/v1/crates/<name>`), user agent
`teigeodata-research`. The date is the crate's last update on crates.io.

| Thing | Version | Checked on | Where |
|---|---|---|---|
| TUF specification | 1.0.37 (2026-10-08) | 2026-11-06 | theupdateframework.github.io/specification |
| tough | 0.24.0 (2026-07-10) | 2026-11-06 | crates.io API, docs.rs, GitHub releases |
| tuf (rust-tuf) | 0.2.0 (2025-10-20) | 2026-11-06 | crates.io API, GitHub README |
| sigstore | 0.15.0 (2026-11-05) | 2026-11-06 | crates.io API |
| minisign / minisign-verify | 0.10.0 / 0.3.0 (2026-09-25) | 2026-11-06 | crates.io API |
| ed25519-dalek | 3.0.0 (2026-07-06) | 2026-11-06 | crates.io API |
| blake3 | 1.8.7 (2026-08-20) | 2026-11-06 | crates.io API |
| bao-tree | 0.17.0 (2026-10-22) | 2026-11-06 | crates.io API, docs.rs |
| bao | 0.13.1 (2025-04-07) | 2026-11-06 | crates.io API |
| abao | 0.2.0 (2023-02-27) | 2026-11-06 | crates.io API |
| iroh-blobs | 0.104.1 (2026-11-04) | 2026-11-06 | crates.io API |
| reqwest | 0.13.5 (2026-09-08) | 2026-11-06 | crates.io API, CHANGELOG |
| hyper / hyper-util | 1.12.0 / 0.1.21 | 2026-11-06 | crates.io API |
| ureq | 3.5.1 (2026-11-02) | 2026-11-06 | crates.io API |
| rustls / rustls-platform-verifier | 0.23.46 / 0.7.1 | 2026-11-06 | crates.io API |
| aws-lc-rs / ring | 1.19.0 / 0.17.14 | 2026-11-06 | crates.io API |
| quinn / h3 | 0.11.12 / 0.0.8 | 2026-11-06 | crates.io API |
| tokio | 1.54.1 | 2026-11-06 | crates.io API |
| fastcdc | 5.0.0 (2026-08-22) | 2026-11-06 | crates.io API |
| zstd / ruzstd | 0.14.3 / 0.9.1 | 2026-11-06 | crates.io API |
| qbsdiff / bsdiff / bidiff | 1.4.4 / 0.2.1 / 1.0.0 (2020) | 2026-11-06 | crates.io API |
| xdelta3 / vcdiff-decoder | 0.1.5 (2019) / 1.0.0 (2024) | 2026-11-06 | crates.io API |
| zsync-rs | 0.3.0 (2026-10-26) | 2026-11-06 | crates.io API |
| memmap2 | 0.9.12 | 2026-11-06 | crates.io API |
| fs4 / tempfile / atomicwrites | 1.1.0 / 3.27.0 / 0.4.4 | 2026-11-06 | crates.io API |
| backon / reqwest-retry | 1.6.0 / 0.9.1 | 2026-11-06 | crates.io API |
| serde_json / postcard | 1.0.154 / 1.1.3 | 2026-11-06 | crates.io API |
| zstd CLI (local) | Homebrew build present; not benchmarked | 2026-11-06 | `/opt/homebrew/bin/zstd` |
| GitHub release asset behaviour | live probe (redirect, ranges, CORS, ETag) | 2026-11-06 | curl against `BurntSushi/ripgrep` 14.1.1 asset |
| Browser support (Background Fetch, OPFS, StorageManager) | MDN browser-compat-data `main` | 2026-11-06 | github.com/mdn/browser-compat-data |
| Organic Maps catalog fields | `master` | 2026-11-06 | `tools/python/post_generation/hierarchy_to_countries.py` |
| OsmAnd index | live | 2026-11-06 | `download.osmand.net/get_indexes?xml` |

## 9. Sources

- [The Update Framework specification 1.0.37](https://theupdateframework.github.io/specification/latest/), TUF project, modified 2026-10-08, accessed 2026-11-06.
- [awslabs/tough](https://github.com/awslabs/tough) and [tough docs](https://docs.rs/tough/latest/tough/), AWS, accessed 2026-11-06.
- [theupdateframework/rust-tuf](https://github.com/theupdateframework/rust-tuf), accessed 2026-11-06.
- [Sigstore security model](https://docs.sigstore.dev/about/security/), Sigstore, accessed 2026-11-06.
- [Immutable releases are now generally available](https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/), GitHub, 2025-10-28.
- [Minisign](https://jedisct1.github.io/minisign/), Frank Denis, accessed 2026-11-06.
- [About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases), GitHub Docs, accessed 2026-11-06.
- [Updated rate limits for unauthenticated requests](https://github.blog/changelog/2025-05-08-updated-rate-limits-for-unauthenticated-requests/), GitHub, 2025-05-08 (via search result; page not fetched).
- [Zstandard as a patching engine](https://github.com/facebook/zstd/wiki/Zstandard-as-a-patching-engine), facebook/zstd wiki, edited 2020-12-17.
- [Smaller is faster (and safer too)](https://blog.chromium.org/2009/07/smaller-is-faster-and-safer-too.html), Chromium Blog, Stephen Adams, 2009-07-15.
- [Software updates: Courgette](https://www.chromium.org/developers/design-documents/software-updates-courgette), Chromium design docs, accessed 2026-11-06.
- [FastCDC, USENIX ATC 2016](https://www.usenix.org/conference/atc16/technical-sessions/presentation/xia), Xia et al.; [TPDS 2020 version](https://ranger.uta.edu/~jiang/publication/Journals/2020/2020-IEEE-TPDS(Wen%20Xia).pdf).
- [folbricht/desync](https://github.com/folbricht/desync), accessed 2026-11-06.
- [Bao specification](https://github.com/oconnor663/bao/blob/master/docs/spec.md), Jack O'Connor, accessed 2026-11-06.
- [BLAKE3](https://github.com/BLAKE3-team/BLAKE3), BLAKE3 team, accessed 2026-11-06.
- [bao-tree docs](https://docs.rs/bao-tree/latest/bao_tree/), n0, accessed 2026-11-06.
- [reqwest CHANGELOG](https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md), accessed 2026-11-06.
- [Downloading files in the background](https://developer.apple.com/documentation/foundation/downloading-files-in-the-background), Apple, accessed 2026-11-06.
- [URLSessionConfiguration.background(withIdentifier:)](https://developer.apple.com/documentation/foundation/urlsessionconfiguration/background(withidentifier:)) and [isDiscretionary](https://developer.apple.com/documentation/foundation/urlsessionconfiguration/isdiscretionary), Apple, accessed 2026-11-06.
- [Encrypting your app's files](https://developer.apple.com/documentation/uikit/encrypting-your-app-s-files), [FileProtectionType](https://developer.apple.com/documentation/foundation/fileprotectiontype), Apple, accessed 2026-11-06.
- [isExcludedFromBackupKey](https://developer.apple.com/documentation/foundation/urlresourcekey/isexcludedfrombackupkey) and [volumeAvailableCapacityForImportantUsageKey](https://developer.apple.com/documentation/foundation/urlresourcekey/volumeavailablecapacityforimportantusagekey), Apple, accessed 2026-11-06.
- [User-initiated data transfer](https://developer.android.com/develop/background-work/background-tasks/uidt), Android Developers, accessed 2026-11-06.
- [Google Maps improved download reliability by 10% using UIDT](https://android-developers.googleblog.com/2024/09/google-maps-improved-download-reliability-user-initiated-data-transfer-api.html), Android Developers Blog, 2024-09-03.
- [Android 15 behaviour changes: dataSync timeout](https://developer.android.com/about/versions/15/behavior-changes-15), Android Developers, accessed 2026-11-06.
- [AssetManager.openFd](https://developer.android.com/reference/android/content/res/AssetManager#openFd(java.lang.String)) and [StorageManager](https://developer.android.com/reference/android/os/storage/StorageManager), Android Developers, accessed 2026-11-06.
- [Storage quotas and eviction criteria](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria), MDN, accessed 2026-11-06.
- [Background Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Background_Fetch_API), MDN, and [browser-compat-data](https://github.com/mdn/browser-compat-data), accessed 2026-11-06.
- [std::fs::rename](https://doc.rust-lang.org/std/fs/fn.rename.html), Rust docs; [std unix fs source](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/unix.rs), accessed 2026-11-06.
- [fsync(2)](https://www.man7.org/linux/man-pages/man2/fsync.2.html), Linux man-pages, accessed 2026-11-06.
- [Mapbox offline guide](https://docs.mapbox.com/ios/maps/guides/offline/), [Manage offline data](https://docs.mapbox.com/ios/maps/guides/offline/manage-offline-data/), [Offline concepts](https://docs.mapbox.com/android/maps/guides/offline/concepts/), [TileRegionLoadProgress](https://docs.mapbox.com/flutter/maps/api/latest/mapbox_maps_flutter/TileRegionLoadProgress-class.html), Mapbox, accessed 2026-11-06.
- [HERE SDK: Get started with offline maps](https://docs.here.com/here-sdk/docs/android-offline-maps.md), HERE, accessed 2026-11-06.
- [Organic Maps generator: hierarchy_to_countries.py](https://github.com/organicmaps/organicmaps/blob/master/tools/python/post_generation/hierarchy_to_countries.py), Organic Maps, accessed 2026-11-06.
- [Organic Maps discussion #8173 (map data updates)](https://github.com/organicmaps/organicmaps/discussions/8173), accessed 2026-11-06.
- [OsmAnd index](https://download.osmand.net/get_indexes?xml), OsmAnd, live, accessed 2026-11-06.
- Live probes with curl 8.7.1 of `github.com/BurntSushi/ripgrep/releases/download/14.1.1/…`, `release-assets.githubusercontent.com`, `api.github.com` and `rust-lang.github.io`, 2026-11-06.
