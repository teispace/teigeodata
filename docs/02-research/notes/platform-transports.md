---
title: "Platform transports: background downloads on iOS, Android and the web"
status: research
updated: 2026-11-11
owner: sync
revisit: 2027-05-01
---

# Platform transports: background downloads on iOS, Android and the web

## 1. Question

Sync gate 14 asks for iOS, Android and web prototypes. A pack install must survive the app
being suspended or killed while a large download runs. Each platform does that with its own
download service: a background `URLSession` on iOS, a user-initiated data transfer (UIDT) job
on Android 14 and later, and `fetch` into the origin-private file system (OPFS) in a browser.
None of these calls back into our code for each response. So how does the Rust installer
(`fetch::run` over a `Transport`), which asks for one range at a time and checks it, work
through them without a second install path? And what must each prototype measure?

## 2. Summary and recommendation

**Two passes over the one installer (D-184).** The installer runs over a transport that serves
only bytes the platform has already downloaded (`fetch::Downloads`). Where bytes are missing,
it does not fail at the first one. It collects every range it still needs, after resume and the
reuse of the old release, and returns them as `FetchError::Needs(ranges)`. The platform
downloads those ranges with its own service, at its own pace, in the background. Then the
installer runs again over what arrived. Every frame is still checked against the signed hashes
before it is written, and the staging file keeps what already checked (resume, D-169). So a
platform transport is only "download these byte ranges of this URL into these files". It holds
no trust and no format knowledge.

The rounds of a typical install:

| Round | Needs | Size | Where it runs |
|---|---|---|---|
| 1 | the seek table and the head frame | a few KB | foreground |
| 2 | `BLOCK_HASHES`, when a section is 1 MiB or more and some frames may be reused | ≤ 1 MiB | foreground |
| 3 | every frame still to do | the install | background |
| 4 | nothing: the pack is committed | | |

A first install with every component can skip the rounds: the platform downloads the whole
asset in one background task, and one pass installs it.

**Measured by each prototype:** the platform's service fetches ranged requests (206 with the
right `Content-Range`), survives suspension, and its files feed round 3. The one claim no source
settles is whether a background `URLSession` task honours a hand-set `Range` header. Measured
on macOS (2026-11-11): it does, also across a redirect. The macOS prototype runs the same `nsurlsessiond`
background machinery as iOS, so it answers that first. Then the iOS simulator, then Android.

The web prototype needs a mirror that sends CORS headers for `Range`: GitHub's release asset
host sends none (§3.3). That is Q-014, an account only the maintainer can open. The browser
mechanics (ranged `fetch` into OPFS through a sync access handle in a worker) can be prototyped
against a localhost server that sends the headers.

## 3. Findings

### 3.1 iOS and macOS: background `URLSession`

- Background sessions run in a system daemon. The app gets the finished file through
  `urlSession(_:downloadTask:didFinishDownloadingTo:)` and must move it before returning. A
  relaunched app reconnects through `handleEventsForBackgroundURLSession`
  ([offline loading and trust](offline-loading-and-trust.md) §3, Apple docs fetched 2026-11-06).
- URLSession's own pause and resume use `Range` and `If-Range` with the ETag, and need a server
  that supports ranges (Apple WWDC23 session 10006; QA1761). Whether a background download task
  keeps a `Range` header the app sets itself is not documented. **Measured on macOS
  (2026-11-11):** it does, and keeps it across a 302 redirect; every response was 206 with the
  exact `Content-Range` ([benches](../benches/phase-4.md#platform-prototypes-d-184)). macOS runs
  background sessions in the same `nsurlsessiond` daemon as iOS. **On the iOS 27.0 simulator
  (2026-11-11, D-185)** the same holds, and an app killed mid-download is relaunched by the
  system in the background to receive the finished file.
- A real app must recreate its background session on every launch, whatever the reason: the
  system's background relaunch carries no arguments of ours, and its events reach only a
  session made with the same identifier. The iOS 27 SDK refuses to launch an app without the
  UIScene life cycle. A task's `taskDescription` survives the kill, so it carries the range.
- iOS 26 adds `BGContinuedProcessingTask`, for user-started work that continues in the
  background with progress the system shows. It reports through `ProgressReporting`. Forum
  threads report it pausing while the device is locked, and a launch handler that stopped firing
  on iOS 26.0 (Apple Developer Forums 796138, 801126, fetched 2026-11-11). It is a candidate for
  round 3's checking work after the files arrive, not for the download.

### 3.2 Android: user-initiated data transfer jobs

- Android 14 (API 34) and later. The app declares `RUN_USER_INITIATED_JOBS` and a
  `JobService`, and builds the job with `setUserInitiated(true)` and, as recommended,
  `setEstimatedNetworkBytes()`. The job must be scheduled while the app is visible. It shows a
  notification, starts at once and may run long. No Jetpack library supports it. Below Android
  14, WorkManager's foreground service is the fallback (developer.android.com, UIDT page,
  fetched 2026-11-11).
- Inside the job, the app does its own HTTP (OkHttp or `HttpURLConnection`), so ranged requests
  are ordinary. The prototype measures that the job runs and survives the app being swiped away.

### 3.3 Web: ranged `fetch` into OPFS

- `FileSystemSyncAccessHandle` (in a dedicated worker) writes at offsets, so a ranged
  response is written where it belongs ([offline loading and trust](offline-loading-and-trust.md)
  §3: Chrome 102, Firefox 111, Safari 15.2 for OPFS).
- GitHub's release asset downloads send no `Access-Control-Allow-Origin`, even after the
  redirect. GitHub's own forum says the release asset bucket does not support CORS (GitHub
  community discussion 45446; corsfix write-up, fetched 2026-11-11). A browser app needs a
  mirror that allows `Range` and exposes `Content-Range` (Q-014).

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A. Two passes over the one installer (`Downloads` transport, `Needs`) | one install path; every byte checked the same way; resume and reuse for free; the platform layer is a dumb downloader | an install takes up to three rounds; files sit on disk between rounds | **Use** |
| B. A planner that lists ranges up front, then a separate "install from parts" path | one round | duplicates the reuse and resume logic; two paths to keep in step | No |
| C. Call back into Rust from each platform's delegate per response | no rounds | impossible in the background: no code runs while the daemon downloads | No |
| D. Whole-file downloads only | simplest | an update fetches the whole pack again; throws away D-174 | Only for a first install |

## 5. What we build ourselves, and why

- `fetch::Downloads`: a `Transport` over files a platform downloaded. Each file holds one byte
  range of the asset (or the whole asset). It serves a `get` the files cover and answers
  `FetchError::Needs` for one they do not.
- `fetch::run` collects every missing range of its last stage instead of stopping at the first,
  and returns them merged into runs. A `Needs` never demotes a mirror.
- `tgd install --downloads DIR` prints what is needed as JSON, or installs when nothing is. The
  prototypes drive the real installer through it, so the platform code in each prototype is
  only the download.

## 6. Impact on the plan

- Phase 4, 4.5: gate 14's prototypes become (a) the core seam above, (b) macOS `URLSession`
  background with `Range`, (c) the iOS simulator, (d) Android UIDT on the emulator, (e) the web
  over localhost; the CORS mirror stays Q-014.
- The bindings (Phase 5) expose `Needs` and `Downloads` to Swift, Kotlin and TypeScript.

## 7. Risks and open items

- A background task that drops `Range` and downloads the whole asset: the prototype detects it
  (`Downloads` checks each file's length against its range). The fallback is D: whole-file
  background downloads.
- Files between rounds take disk: round 3's files are at most the install's wire bytes, and
  they are removed after the commit. The disk-peak formula ([download and delta](download-and-delta.md)
  §3.4) already counts the whole-file case.
- Q-014 (CORS mirror) blocks a browser app against the real hosts, not the prototype.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Xcode | 27.2 | 2026-11-11 | `xcodebuild -version` |
| Swift | 6.4.2 | 2026-11-11 | `swift --version` |
| Android SDK platforms | 31 to 37.0; system image 35 | 2026-11-11 | the local SDK |
| `BGContinuedProcessingTask` | iOS 26.0+ | 2026-11-11 | developer.apple.com |
| UIDT jobs | Android 14 (API 34)+ | 2026-11-11 | developer.android.com |

## 9. Sources

- Apple, *Build robust and resumable file transfers*, WWDC23 session 10006:
  <https://developer.apple.com/videos/play/wwdc2023/10006/>
- Apple, QA1761 *Resumable downloads*: <https://developer.apple.com/library/ios/qa/qa1761/_index.html>
- Apple, `BGContinuedProcessingTask`:
  <https://developer.apple.com/documentation/backgroundtasks/bgcontinuedprocessingtask>;
  forums <https://developer.apple.com/forums/thread/796138>,
  <https://developer.apple.com/forums/thread/801126>
- Android, *User-initiated data transfer*:
  <https://developer.android.com/develop/background-work/background-tasks/uidt>
- GitHub community, *Download release assets and zipball with CORS*:
  <https://github.com/orgs/community/discussions/45446>
- corsfix, *How to fetch files from a GitHub release*: <https://corsfix.com/blog/fetch-github-release>
