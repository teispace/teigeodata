---
title: "Language bindings, packaging, CI and the MCP server"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-01
---

# Language bindings, packaging, CI and the MCP server

## 1. Question

teigeodata ships one API with identical signatures and behaviour in C/C++, Node, browser/WASM (plus Deno, Bun, Cloudflare Workers), Dart/Flutter, Python, JVM, Android, Swift, .NET and Go. A cross-language parity gate holds every binding to the same answers. We need to know:

1. Which binding strategy to use. The current plan is a designed stable C ABI in a boundary crate, an IDL extracted from the Rust source with doc-comment metadata (units, ranges, examples), our own generators (written in Rust) for each language's mechanical layer, and thin hand-written idiomatic layers. The alternatives are Diplomat, UniFFI and its third-party backends, and per-ecosystem tools.
2. The best mechanism and packaging for each target as of November 2026.
3. What GitHub Actions gives a public repo for the build, release and signing matrix.
4. Which MCP revision to target from day 0, the state of the official Rust SDK, and how to design a geodata MCP server.

This note feeds the bindings ADR, the boundary-crate architecture page, the release and CI pipeline, the MCP server design, and the parity gate.

## 2. Summary and recommendation

**Strategy: confirm the plan, with five changes.**

1. **One IDL, two kinds of backend.** The IDL is the single source of truth. Most targets (C/C++, Swift, .NET, Go, Dart, Python) sit on the designed C ABI. **Node and WASM do not go through the C ABI.** For those, the generators emit Rust shim code for napi-rs and wasm-bindgen that calls the core crate directly. Node cannot load a C ABI without a third-party FFI add-on. WASM call cost and JS glue are better handled by wasm-bindgen than by hand-rolled pointer passing. The parity gate still holds because the shims come from the same IDL and share the same core.
2. **JNI for both Android and desktop JVM.** Java FFM is final from Java 22, but Android has no FFM, and many JVM users are still on Java 17/21. One generated JNI layer (Rust side via `jni` 0.22, Kotlin/Java side generated) serves the AAR and the desktop JAR. An FFM backend can come later if users ask for it.
3. **Python via generated PyO3 shims, not ctypes/cffi over the C ABI.** This gives abi3 wheels now and PEP 803 `abi3t` (free-threaded stable ABI) for 3.15+, which PyO3 0.29+ and maturin already support. Exposing the columnar blob through the buffer protocol keeps decoding zero-copy.
4. **Keep the ABI surface deliberately small.** Use opaque handles, POD structs, UTF-8 slices, one columnar result blob (format to be chosen in a spike), error codes plus a last-error accessor, callbacks as C function pointer plus `void*` context, and async only through job handles (poll or completion callback). This keeps the per-language generators to roughly one template set each.
5. **Borrow before building.** Study Diplomat's HIR and its plugin interface, and UniFFI's new `uniffi_parse_rs` (it builds metadata from source without compiling), before writing the IDL extractor. Neither is adopted wholesale (reasons in section 4).

**Per target (detail in section 3):**

| Target | Mechanism | Packaging |
|---|---|---|
| C/C++ | Designed C ABI; header emitted by our generator, cross-checked with cbindgen 0.29 | `.so/.dylib/.dll` plus static libs, header, pkg-config/CMake config, on GitHub Releases |
| Node | napi-rs 3.x (`napi` 3.16.1, `@napi-rs/cli` 3.10.12), Node-API 9 baseline | npm package with per-platform `optionalDependencies` prebuilds; optional `wasm32-wasip1-threads` fallback package |
| Browser/WASM, Deno, Bun, Workers | wasm-bindgen 0.2.130 (now under the `wasm-bindgen` GitHub org) plus binaryen `wasm-opt` (version_133) | npm packages per feature variant (ESM); JSR for Deno |
| Dart/Flutter | C ABI plus generated `@Native` bindings plus `NativeFinalizer`; Dart build hooks (stable since Dart 3.10 / Flutter 3.38) | pub.dev package whose `hook/build.dart` downloads verified prebuilt binaries, with a build-from-source fallback via `native_toolchain_rust` 1.0.7 |
| Python | Generated PyO3 0.30 shims, maturin 1.16, cibuildwheel 4.3 | abi3 wheels (cp310+), abi3t wheels for 3.15t+ |
| JVM | JNI (generated) | JAR with native libs per OS/arch on Maven Central under `com.teispace` |
| Android | JNI (generated), cargo-ndk 4.1, NDK r30 | AAR on Maven Central; 16 KB ELF alignment enforced in CI |
| Swift | C ABI plus module map plus generated Swift wrapper | XCFramework zipped as a SwiftPM `binaryTarget` (CocoaPods trunk becomes read-only on 2026-12-02, so no podspec) |
| .NET | C ABI plus generated `[LibraryImport]` (source-generated P/Invoke), NativeAOT-safe | NuGet package with `runtimes/<rid>/native/` |
| Go | C ABI via cgo (default); purego 0.11 as a `CGO_ENABLED=0` option (spike first) | Go module with prebuilt static libs per platform, or build tag for system lib |

**MCP:** target protocol revision **2026-07-28** (current), with fallback to 2025-11-25 for older clients. Build on `rmcp` **3.9.0** (Apache-2.0, official), which implements 2026-07-28: stateless lifecycle, `server/discover`, sessionless Streamable HTTP, MRTR, `subscriptions/listen`, the tasks extension and caching. Ship stdio first, then stateless Streamable HTTP. Every tool has an `outputSchema`, returns `structuredContent` plus a short text summary, uses bounded result sizes with cursor handles, and exposes datasets and places as resources.

**Unverified items** are marked inline. The main ones are client-side tool-output size limits, the rustc default page size for Android targets, NuGet trusted publishing, and the Cloudflare Workers bundle size limit.

## 3. Findings

### 3.1 Strategy comparison (late 2026)

| Tool | Version (checked 2026-11-06) | Model | Callbacks | Async | Marshalling | Targets | Notes |
|---|---|---|---|---|---|---|---|
| **Own C ABI + IDL + generators** | n/a | Designed C ABI; generators per language | Function pointer plus context (we design it) | Job handle plus poll/callback (we design it) | As cheap as we design it; zero-copy columnar blob possible | All | Highest initial cost; full control of metadata, parity and the blob |
| **Diplomat** | 0.17.0 (2026-10-20), MIT OR Apache-2.0 | Annotated `#[diplomat::bridge]` module produces C ABI plus backends | Yes in some backends (callbacks/traits); coverage varies by backend (**unverified per backend**) | No | Direct C ABI; slices and strings borrowed | C, C++, Dart, JS/TS (WASM), .NET, Kotlin (**JNA**), Python (**nanobind**) | Used by ICU4X. Plugin interface for new languages. JS backend needs the spec wasm C ABI (nightly flag) or a legacy config. No Swift, Go or JNI. |
| **UniFFI** | 0.32.2 (2026-09-22), **MPL-2.0** | Proc-macros or UDL; scaffolding plus per-language bindgen | Foreign traits/callbacks | Yes (futures mapped to Swift async, Kotlin coroutines, Python asyncio) | Serialises compound values to `RustBuffer` | Official: Kotlin (JNA), Swift, Python, Ruby. Unreleased: experimental `uniffi-bindgen-kotlin-jni` | Strong for Swift/Kotlin mobile. Per-call overhead from serialisation and JNA. |
| UniFFI third-party | uniffi-bindgen-go v0.7.1+v0.31.0; uniffi-bindgen-cs v0.11.1+v0.31.0 (both NordSecurity); uniffi-bindgen-java 0.5.2 (IronCoreLabs); uniffi-dart (active, unreleased on crates.io); uniffi-bindgen-react-native 0.31.0-6 | Same | Same | Varies | Same | Go, C#, Java, Dart, RN/WASM | Pinned to UniFFI 0.31 while core is on 0.32.x. Version skew is structural. |
| **BoltFFI** | 0.32.1 (2026-10-30), MIT | Own macros (`#[data]`, `#[export]`), zero-copy where possible | **unverified** | **unverified** | Raw primitives, pointer-shared structs, wire buffer for WASM | Swift, Kotlin, WASM, others (**unverified**) | Created 2026-02. Claims "up to 1000x faster than UniFFI" are **vendor benchmarks, unverified**. Too young to depend on, but the design validates our zero-copy direction. |
| napi-rs | `napi` 3.16.1, `napi-derive` 3.7.0, `@napi-rs/cli` 3.10.12; MIT | Rust macros produce a Node-API addon plus `.d.ts` | ThreadsafeFunction (redesigned in v3) | Rust futures to Promises | Node-API; typed arrays and Buffers can be zero-copy | Node, Bun, Deno (Node-API compatibility, **unverified per runtime**), WASI fallback | v3 announced 2025-07-07; adds `wasm32-wasip1-threads` builds |
| wasm-bindgen | 0.2.130 (2026-10-30), MIT OR Apache-2.0 | Rust macros produce JS glue plus `.d.ts` | JS closures | Rust futures to Promises (`wasm-bindgen-futures`) | Copies at the boundary unless you expose views over linear memory | Browser, Deno, Bun, Node, Workers | Repo moved from `rustwasm` to the `wasm-bindgen` org and is active (last push 2026-11-06). `wasm-pack` 0.15.1 also lives there now. |
| PyO3 / maturin | PyO3 0.30.1, maturin 1.16.0; MIT OR Apache-2.0 | Rust macros produce a CPython extension | Python callables | `pyo3-async-runtimes` (**unverified version**) | Buffer protocol for zero-copy | CPython (abi3, abi3t, free-threaded), PyPy | PyO3 0.30.0 (2026-10-13) supports Python 3.15.0 final; 0.29.0 added abi3t |
| flutter_rust_bridge | 2.13.0, MIT | Codegen of Rust plus Dart over its own wire protocol | Yes | Yes (Dart Futures/Streams) | Serialisation (SSE codec) or direct for simple types | Dart/Flutter only | Productive for apps; hides the ABI we want to own; Dart-only |
| jni-rs | 0.22.4, MIT OR Apache-2.0 | Hand-written JNI in Rust | Manual | Manual | Manual; `DirectByteBuffer` for zero-copy | Android, JVM | Plumbing for our generated JNI layer |
| swift-bridge | 0.1.59 (2026-01-06), Apache-2.0/MIT | Bridge module produces Swift plus C | Yes | Yes | Mostly direct | Swift only | Pre-1.0, slow release cadence |
| csbindgen | 1.9.8, MIT | Generates C# `DllImport` from Rust `extern "C"` | Function pointers | No | Direct | C# only | Useful reference; we generate `LibraryImport` ourselves |
| cbindgen | 0.29.4, MPL-2.0 | Header generator from `extern "C"` | n/a | n/a | n/a | C/C++ | Build-time tool only (MPL does not reach our output). Good cross-check for our emitted header. |
| interoptopus | 0.16.6, MIT | Rust-defined inventory produces backends | Yes | Partial (**unverified**) | Direct | C#, C, Python (**unverified current list**) | MSRV 1.93; small community |
| safer_ffi | 0.1.13, MIT | Safe `#[ffi_export]` plus header generation | Yes (closures) | No | Direct | C/C++ (headers), C# header gen | Helps write the boundary crate safely; not a multi-language generator |

**Assessment by criterion:**

- **Code volume.** UniFFI and Diplomat need the least first-party code (annotate, then generate). Our own approach costs an IDL extractor plus about 9 template sets. Each template set is maybe 1-3k lines if the ABI surface stays small (**estimate, to be measured in spike S1**). Per-ecosystem tools need N separate hand-written binding crates, which is the worst for parity.
- **Callbacks.** All serious options support them. Thread affinity is the hard part: Dart isolates (callbacks must be `NativeCallable.listener` from other threads), Node (ThreadsafeFunction), Python (GIL or free-threaded), JNI (`AttachCurrentThread`). No generator solves this for us. We must specify it in the ABI.
- **Async.** Only UniFFI, napi-rs, wasm-bindgen, PyO3 and flutter_rust_bridge map Rust futures natively. Diplomat has none. Our core is mostly synchronous (offline lookup); only the download manager is async. A job-handle plus completion-callback ABI is enough, and each binding wraps it in its own idiom (Promise, Future, `async`, `Task`, coroutine, channel).
- **Marshalling cost.** UniFFI serialises compound values; BoltFFI's (vendor) numbers and UniFFI's own JNI experiment both point at that overhead. For a columnar result, the right design is one blob (pointer plus length plus release function) decoded zero-copy in each language. No off-the-shelf generator does that, but Diplomat and our own approach allow it.
- **Idiomatic output.** UniFFI's Swift/Kotlin output is idiomatic. Diplomat's C++/JS/Dart output is good. Ours is as good as our templates plus the hand-written layer.
- **Maintenance risk.** UniFFI is MPL-2.0 and its third-party backends lag one minor version behind. Diplomat is driven by ICU4X needs (no async, Kotlin via JNA). Owning the generators puts the risk on us, but it is bounded by keeping the ABI small and the generators template-driven.

### 3.2 Node

- Node release lines on 2026-11-06: **v26 is Active LTS (since 2026-10-28)**, v24 LTS is in maintenance (since 2026-10-20, EOL 2028-04-30), v22 is in maintenance (EOL 2027-04-30), and v20 reached EOL on 2026-04-30. v27 is in alpha (Node release schedule JSON).
- Node-API: the highest version documented is **10** (Node 22.14+, 23.6+ and later). Version 9 covers Node 18.17+/20.3+/21+. Node-API 9+ is versioned independently of Node (Node-API docs).
- napi-rs v3 (announced 2025-07-07) adds WebAssembly builds (`wasm32-wasip1-threads`) from the same code, a redesigned ThreadsafeFunction, and replaces the large cross-compilation Docker images (napi.rs blog).
- **Packaging:** use the napi-rs CLI layout. That is one main package plus per-platform packages (`@teispace/teigeodata-linux-x64-gnu`, `-linux-x64-musl`, `-linux-arm64-gnu`, `-darwin-arm64`, `-darwin-x64`, `-win32-x64-msvc`, `-win32-arm64-msvc`) as `optionalDependencies`, plus an optional `-wasm32-wasi` fallback. Publish with npm provenance from GitHub Actions OIDC.
- **Recommendation:** build against Node-API 9 (covers all supported LTS lines). Generate the `#[napi]` shim from the IDL. Hand the columnar blob over as an `ArrayBuffer` external, so JS decodes it zero-copy via typed-array views.

### 3.3 WASM (browser, Deno, Bun, Cloudflare Workers)

- **wasm-bindgen** is maintained under the new `github.com/wasm-bindgen` org after the rustwasm org wind-down. 0.2.130 was released on 2026-10-30. `wasm-pack` 0.15.1 (2026-10-30) also moved there (GitHub API, checked 2026-11-06).
- **Component model / WASI:** WASI **0.3.0 was ratified and released on 2026-06-11** (v0.3.2 on 2026-10-13). It adds native async (`future<T>`, `stream<T>`) to the component model (WASI release notes). Wasmtime is at v50.0.1. Browsers still do not run components natively; they need `jco` transpilation (`@bytecodealliance/jco` 1.41.0). `cargo-component` is still labelled experimental (last release 0.21.1, 2025-04). `wit-bindgen` 0.63.0 is current. **Conclusion:** the component model suits server-side runtimes, not our browser package. Revisit in 2027.
- **Size:** run binaryen `wasm-opt -Oz` (version_133) after `wasm-bindgen`. Also use `opt-level="z"`, `lto="fat"`, `codegen-units=1` and `panic="abort"`, strip names, and avoid `format!`-heavy paths. Measure gz/brotli size per feature variant (spike S3).
- **Cloudflare Workers:** script size limits apply (roughly 3 MB compressed on Free and 10 MB on paid, **unverified, check Cloudflare docs before the spike**). The feature-variant builds matter most here.
- **Diplomat's note:** the spec-compliant C ABI for `wasm32-unknown-unknown` still needs a nightly flag or a legacy mode in Diplomat's JS backend (Diplomat README). That is one more reason to use wasm-bindgen for WASM rather than C-ABI-over-WASM.
- **Recommendation:** generate wasm-bindgen shims from the IDL. Return columnar data as views into linear memory (or copied `Uint8Array`s for safety across `memory.grow`; to be measured). Publish one npm package per feature variant. Flutter web consumes the same WASM through Dart JS interop (spike S5).

### 3.4 Dart and Flutter

- **Build hooks are stable.** The Dart SDK changelog for **3.10.0** says: "Support for hooks -- formerly know as native assets -- are now stable." Flutter 3.38 (Dart 3.10) made `flutter create --template=package_ffi` the recommended way to bind native code (docs.flutter.dev, "Bind to native code", 3.47 docs). No flag is needed.
- Current releases: **Flutter 3.47.9 (2026-10-29) with Dart 3.13.7 (2026-10-22).** Dart 3.13 (2026-08-12) added **link hooks** and recorded-usage (`record_use`) tree-shaking of native symbols (dart.dev/tools/hooks). That fits our feature-variant design: a link hook can drop unused native code. Dart 3.13.3 fixed Windows-to-Linux cross-compilation with link hooks.
- Packages: `hooks` 2.2.0, `code_assets` 2.2.0, `ffigen` 23.0.0, `ffi` 2.2.0. Only `CodeAsset` is supported; "more asset types are planned", so **data assets are not stable** (dart.dev/tools/hooks). Geodata files must be shipped some other way (Flutter assets or the download manager).
- Hook limits: hooks run semi-hermetically (most env vars stripped), can only output assets in their own package, and hook dependencies must be normal `dependencies`.
- `@Native` external functions resolve against the hook-provided code asset, so no `DynamicLibrary.open` per OS is needed. Use `NativeFinalizer` to release handles and the columnar blob, and `NativeCallable.listener` for callbacks from Rust threads.
- `native_toolchain_rust` 1.0.7 (pub.dev, verified publisher gsconrad.com, 2026-09-10) builds Rust in a hook on Android, iOS, Linux, macOS and Windows. It needs rustup and a pinned `rust-toolchain.toml`. Third-party, single maintainer: **use it only as a fallback.** Our default hook downloads prebuilt, checksummed binaries from GitHub Releases, so app developers do not need Rust installed.
- `flutter_rust_bridge` 2.13.0 now ships hook integration (`flutter_rust_bridge_hooks` in beta), but it imposes its own wire format and is Dart-only. Not chosen.

### 3.5 Python

- Current releases: 3.15.2, 3.14.10, 3.13.18 (all 2026-10-25, python.org API).
- CPython declared free-threading supported from 3.14. PyO3 0.29.0 dropped 3.13t and kept 3.14t+ (PyO3 changelog).
- **PEP 803 `abi3t`** (stable ABI for free-threaded builds) was accepted for 3.15 (discuss.python.org thread). PyO3 0.29.0 added `abi3t` and `abi3t-py315` features. maturin supports abi3t for 3.15 with PyO3 0.29 and includes `SOABI_PLATFORM` in 3.15 stable-ABI filenames (maturin changelog).
- Tooling: maturin 1.16.0, cibuildwheel 4.3.5, maturin-action v1.51.0.
- **Recommendation:** two wheel families per platform. **cp310-abi3** covers all GIL builds from 3.10. **cp315-abi3t** covers free-threaded 3.15+; 3.14t needs a version-specific `cp314t` wheel if we support it. Mark the module `gil_used = false` only after the thread-safety review. Expose the columnar blob via the buffer protocol (works with numpy and pyarrow without a hard dependency). Publish with PyPI trusted publishing and attestations.

### 3.6 JVM

- JDK: most recent feature release **27**, most recent LTS **25** (Adoptium API). FFM (`java.lang.foreign`) has been final since JDK 22; `jextract` is still developed out-of-tree (last commit 2026-10-14).
- **Maven Central** (Central Portal; OSSRH is retired) requires: `-sources.jar` and `-javadoc.jar` for every JAR (placeholders allowed), `.md5` and `.sha1` for every file, an `.asc` GPG signature for every file, and POM name, description, URL, licence, developers and SCM. Versions cannot end in `-SNAPSHOT` (central.sonatype.org requirements).
- **Namespace `com.teispace`:** verified with a DNS TXT record on exactly `teispace.com`. Sub-groups such as `com.teispace.geodata` fall under it. Do not click Confirm before the TXT record exists, because the NXDOMAIN gets cached (central.sonatype.org namespace page). **Open item:** confirm the org controls `teispace.com`; otherwise the fallback is `io.github.teispace`.
- **Recommendation:** use JNI for the desktop JVM too (see section 2). The JAR bundles native libs under `META-INF/native/<os>-<arch>/` and extracts them on load. Ship a Kotlin-first API with Java-friendly overloads. FFM backend: deferred.

### 3.7 Android

- No FFM on Android, so JNI it is. Options are hand-written JNI in Rust via `jni` 0.22.4, UniFFI's Kotlin over JNA (slower; UniFFI itself is experimenting with a JNI backend), or our generator emitting the JNI shim. Choose the last.
- `cargo-ndk` 4.1.2 (2025-08-09); NDK **r30** (2026-09-08).
- **16 KB pages:** all apps targeting Android 15 (API 35)+ must support 16 KB page sizes on 64-bit devices on Google Play. **From 2027-02-01, updates that do not support 16 KB cannot be released** (developer.android.com page-sizes guide). NDK r28+ compiles 16 KB-aligned by default. AGP 8.5.1+ handles zip alignment. The guide also adds a RELRO end-alignment check. Rust links through the NDK clang driver (via cargo-ndk), which sets `max-page-size=16384` for AArch64 Android. Whether rustc's own target spec does so is **unverified**, so we pass `-C link-arg=-Wl,-z,max-page-size=16384` explicitly for `arm64-v8a` and `x86_64`, and CI checks every `.so` with `llvm-readelf -Wl` (LOAD align ≥ 2**14, RELRO rule) plus `zipalign -c -P 16`.
- **Packaging:** an AAR with `jniLibs/{arm64-v8a,armeabi-v7a,x86_64}` (x86 only if asked) on Maven Central, `minSdk` to be decided (21 or 24), consumer ProGuard rules for JNI entry points. Background downloads go through WorkManager in the Kotlin layer, which calls the Rust download manager's pluggable transport.

### 3.8 Swift

- Swift **6.4.2** (2026-10-30). Swift 6 strict concurrency means handle types must be `final class ... : @unchecked Sendable` with internal locking, or actors. Callbacks from Rust threads must hop to the caller's executor. The generated wrapper must compile cleanly in Swift 6 language mode.
- Mechanism: a C header plus `module.modulemap` inside an XCFramework (iOS device, iOS simulator arm64/x86_64, macOS universal, optionally Mac Catalyst, visionOS, tvOS), distributed as a zipped SwiftPM `binaryTarget` with a checksum. Plus a source target with the Swift wrapper.
- **CocoaPods:** trunk becomes **permanently read-only on 2026-12-02**, after a test window 2026-11-01..07 (CocoaPods blog, "CocoaPods Specs Repo"). No podspec. SwiftPM only.
- Background downloads go through a `URLSession` background configuration in the Swift layer, which plugs in as the download manager's transport. Ship a privacy manifest (`PrivacyInfo.xcprivacy`) in the XCFramework.

### 3.9 .NET

- .NET **10** is the current LTS (Microsoft.NETCore.App.Ref 10.0.14). .NET 11 is at RC 3.
- Mechanism: `[LibraryImport]` (source-generated P/Invoke, trimming and NativeAOT compatible), `SafeHandle` subclasses for handles, `ReadOnlySpan<byte>` / `UnmanagedMemoryManager` views over the columnar blob, and `[UnmanagedCallersOnly]` function pointers for callbacks.
- Packaging: one NuGet package with `runtimes/{win-x64,win-arm64,linux-x64,linux-arm64,linux-musl-x64,osx-arm64,osx-x64}/native/`. iOS/Android for .NET MAUI is a later extra. Whether NuGet trusted publishing via OIDC is available is **unverified**; assume an API key until checked.

### 3.10 Go

- Go **1.27.3** and 1.26.10 are current.
- **cgo** is the reference path: we link a static lib per GOOS/GOARCH. It needs a C toolchain for users and cross-compiles poorly.
- **purego** v0.11.3 (2026-11-01) calls C functions without cgo (dlopen at runtime). It works with `CGO_ENABLED=0` but needs a shared library at runtime, and struct-by-value and callback support is narrower (**unverified for our ABI shapes; spike S1 covers it**).
- **Recommendation:** cgo by default, with build-tagged purego as an option if the spike passes. Ship prebuilt static libs in a separate module, or document `go generate`-time download.

### 3.11 C/C++

- The designed C ABI is the product. Our generator emits `teigeodata.h` from the IDL. cbindgen 0.29.4 runs in CI on the boundary crate, and any diff from the emitted header fails the build. A header-only C++17 wrapper (RAII handles, `std::span` over the blob) is generated too.
- Packaging: tarballs/zips per platform on GitHub Releases with shared and static libs, a CMake package config and a pkg-config file. vcpkg and Conan ports come later.

### 3.12 GitHub Actions for a public repo (2026)

- **Standard runners (public repos, free and unlimited):** Linux x64 4 CPU/16 GB/14 GB SSD (`ubuntu-latest`, `ubuntu-24.04`, `ubuntu-22.04`, `ubuntu-26.04`); Linux arm64 4 CPU/16 GB (`ubuntu-24.04-arm`, `ubuntu-22.04-arm`, `ubuntu-26.04-arm`); Windows x64 4 CPU/16 GB (`windows-latest`, `windows-2025`, `windows-2025-vs2026`, `windows-2022`); Windows arm64 4 CPU/16 GB (`windows-11-arm`, `windows-11-vs2026-arm`); macOS arm64 (M1) 3 CPU/7 GB (`macos-latest`, `macos-15`, `macos-26`, `xcode-27`); macOS Intel 4 CPU/14 GB (`macos-15-intel`, `macos-26-intel`); plus a 1-CPU `ubuntu-slim` container (GitHub docs, github-hosted runners).
- **Limits:** job 6 h on hosted runners; workflow run 35 days; matrix 256 jobs per run; Free plan concurrency 20 jobs, of which **5 macOS**; cache 10 GB per repo; artifact storage quota 500 MB (Free); logs and artifacts kept 90 days by default (GitHub docs, limits and artifact pages). Release assets: each file under 2 GiB, up to 1,000 assets per release, no total size or bandwidth limit (GitHub docs, about releases).
- The 5-macOS concurrency limit is our bottleneck (XCFramework, macOS dylibs, Flutter iOS checks). Batch Apple slices into one macOS job.
- **Cross-compilation:** `cargo-zigbuild` 0.24.1 (2026-11-04) targets a chosen glibc version (for example `x86_64-unknown-linux-gnu.2.17`) and musl from one Linux runner. `cross` last released 0.2.5 in 2023-02; main is still active but release cadence makes it a second choice. Native arm64 Linux and Windows-arm runners remove most of the need for emulation.
- **Release tooling:** `release-plz` 0.4.3 for crate versioning, changelogs and crates.io publishing. `cargo-dist` 0.34.0 for the MCP server binary and CLI (installers, Homebrew, checksums). Library artifacts per ecosystem (npm, PyPI, Maven, NuGet, pub, SwiftPM zip) need our own workflows; cargo-dist does not cover them.
- **Signing and attestations:** `actions/attest-build-provenance` v4.2.2 (wraps `actions/attest` v4.2.2). It produces SLSA build-provenance attestations signed through public-good Sigstore for public repos, verified with `gh attestation verify`. cosign v3.1.4 handles detached signatures where needed. Use trusted publishing (OIDC) wherever the registry supports it: crates.io, npm (with provenance), PyPI (with PEP 740 attestations) and pub.dev (GitHub Actions OIDC). Maven Central needs GPG; NuGet is **unverified**.
- **OpenSSF Scorecard:** `ossf/scorecard-action` v2.5.0 (scorecard v5.6.0, 2026-10-12). Pin actions by SHA, set least-privilege `permissions:`, add a branch-protection-friendly setup and a `SECURITY.md`.
- Per the maintainers' own policy, CI for this **public** repo runs on GitHub, not locally under `act`.

### 3.13 MCP

**Protocol revision.** The **current revision is 2026-07-28** (modelcontextprotocol.io/specification/versioning). Key changes from 2025-11-25 (spec changelog):

- **Stateless.** The `initialize`/`initialized` handshake is gone. Every request carries `io.modelcontextprotocol/protocolVersion` and `clientCapabilities` in `_meta`. Servers **MUST** implement **`server/discover`**. A version mismatch returns `UnsupportedProtocolVersionError` (-32022).
- **Streamable HTTP without sessions.** No `Mcp-Session-Id`, no GET stream, no `Last-Event-ID` resumption. POST only; responses are JSON or a per-request SSE stream; closing the stream means cancellation. Required headers are `MCP-Protocol-Version`, `Mcp-Method` and `Mcp-Name` (validated against the body; mismatch returns -32020). `x-mcp-header` mirrors tool parameters into `Mcp-Param-*` headers. Servers must validate `Origin` and should bind to localhost when local.
- **`subscriptions/listen`** replaces GET and `resources/subscribe`. `ping`, `logging/setLevel` and `roots/list_changed` are removed; the log level is now per request in `_meta`.
- **Multi Round-Trip Requests (MRTR).** Servers return `resultType: "input_required"` with `inputRequests` (elicitation, sampling, roots) instead of sending server-to-client requests. All results carry `resultType`. Cross-call state goes in explicit server-minted handles passed as tool arguments.
- **Tasks** moved out of core into the official extension `io.modelcontextprotocol/tasks` (polling `tasks/get`, `tasks/update`, `tasks/cancel`; `tasks/list` removed).
- **Elicitation** stays (form and URL modes, via MRTR); `notifications/elicitation/complete` is removed.
- **Structured output:** `inputSchema`/`outputSchema` accept any JSON Schema 2020-12. `structuredContent` can be any JSON value and must conform to `outputSchema` if one is given. Servers SHOULD also return the serialised JSON as text for backwards compatibility.
- **Caching:** `ttlMs` and `cacheScope` are required on list and read results. `tools/list` should come back in deterministic order.
- **Resources:** resource-not-found is now -32602.
- **Auth:** RFC 9207 `iss` validation; Client ID Metadata Documents are preferred and Dynamic Client Registration is deprecated; credentials are bound to their issuer.
- **Deprecated:** Roots, Sampling, Logging, the HTTP+SSE transport, and `includeContext` `thisServer`/`allServers`. There is a 12-month minimum deprecation window.
- Extensions field in capabilities. `rmcp` 3.9.0 also supports a Skills extension (SEP-2640).

**Rust SDK.** `rmcp` **3.9.0** (2026-11-05), Apache-2.0, MSRV 1.88, repo `modelcontextprotocol/rust-sdk`. rmcp **3.0.0** (2026-07-28) "adds support for MCP 2026-07-28". It uses sessionless Streamable HTTP with a fresh handler per request (`stateful_mode` was renamed `legacy_session_mode`), stateless lifecycle and discovery, `subscriptions/listen`, MRTR and `resultType`. The README says it "implements the stable MCP 2026-07-28 specification while remaining fully compatible with the 2025-11-25 release and earlier", and that it covers the tasks extension (`TaskManager`), caching and the HTTP routing headers. Release cadence is about weekly (3.1.0 to 3.9.0 between 2026-07-31 and 2026-11-05), and 3.1.0 records Tier 1 conformance work. The TypeScript SDK is at 1.32.2 and the Python `mcp` package at 2.3.0, which is useful for conformance cross-testing.

**Geodata MCP server best practices (from the spec plus our needs):**

- **Tools, few and orthogonal**, named `geodata.search`, `geodata.reverse`, `geodata.nearby`, `geodata.lookup`, `geodata.datasets` (dots allowed; 1-128 chars). Each tool description states units (metres, WGS84 degrees), ranges, defaults and an example, taken straight from the IDL doc-comment metadata, so the MCP schema and every binding share one source.
- `annotations.readOnlyHint: true` on query tools. Download or install tools are separate and marked non-read-only.
- **Always give an `outputSchema`.** Return `structuredContent` (a compact JSON array of places with id, name, kind, lat, lon, admin path, distance_m, score) plus a short text summary. Do not dump a GeoJSON blob into text.
- **Bounded output:** default `limit` 10, max 50, enforced in schema (`maximum`). Paginate with an opaque `cursor` handle (stateless: encode query plus offset and sign it, or keep it server-side with a TTL stated in the tool description). Client-side caps on tool output vary by host (some cap around 25k tokens by default, **unverified per client**), so keep a typical response under about 4-8k tokens and offer a `fields` selector.
- **Resources and resource templates:** `teigeodata://dataset/{id}` (licence, attribution, coverage, version), `teigeodata://place/{id}` (full record, GeoJSON as `application/geo+json`). Tools return `resource_link`s for large payloads instead of embedding them. Set `ttlMs`/`cacheScope: "public"` on dataset metadata.
- **Attribution and licences** (ODbL and similar) go into every result's `_meta` or a dataset resource link, so the host can show them.
- **Downloads:** a `geodata.download` tool that returns a task (tasks extension) when the client supports it, and otherwise a handle plus a `geodata.download_status` tool. It asks for size confirmation through MRTR elicitation (form mode), and only when the client declares elicitation.
- **Transports:** stdio for local agents first. Then Streamable HTTP in stateless mode (JSON responses, SSE only for progress), bound to 127.0.0.1 by default, with `Origin` validation. Remote auth follows the 2026-07-28 authorization spec later.
- Do not adopt Sampling, Roots or Logging (deprecated). Logs go to stderr or OpenTelemetry.
- Run the official MCP conformance suite in CI (rmcp already tracks it), testing both 2026-07-28 and 2025-11-25 clients.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Own C ABI + IDL + generators (plan) | One contract; metadata (units, ranges, examples) flows into docs, bindings and MCP schemas; zero-copy columnar blob; parity by construction; covers all targets incl. Go and JNI | Upfront cost; we own async/callback/thread rules and every template | Ours (Apache-2.0) | New | **Adopt, with Node/WASM/Python via generated Rust shims** |
| Diplomat | C-ABI-centric like us; plugin interface; borrowed slices; C++/JS/Dart/.NET output; proven by ICU4X | No async; Kotlin via JNA; Python via nanobind (C++ build); no Swift/Go/JNI; no custom metadata channel; WASM JS backend needs a nightly ABI flag | MIT OR Apache-2.0 | 0.17.0 | Reject as dependency; **study HIR and plugin design** |
| UniFFI (+ third-party) | Best async/callback story; idiomatic Swift/Kotlin; large user base (Mozilla) | Serialising `RustBuffer`; JNA on Kotlin; MPL-2.0; third-party backends lag (0.31 vs 0.32); no first-party Node/WASM/Dart/C#/Go | MPL-2.0 | 0.32.2 | Reject; watch `uniffi_parse_rs` and the JNI Kotlin backend |
| BoltFFI | Zero-copy design, claims large speedups | 9 months old; benchmarks are vendor-run; scope unclear | MIT | 0.32.1 | Watch |
| Per-ecosystem tools only (napi-rs, wasm-bindgen, PyO3, frb, jni-rs, swift-bridge, csbindgen) | Best idioms per language; mature for Node/WASM/Python | N hand-written binding crates; parity drifts; no shared metadata | MIT / MIT-Apache | Mature (varied) | Use napi-rs, wasm-bindgen, PyO3 and jni-rs as **runtimes under our generator**, not as the strategy |
| interoptopus / safer_ffi | Safe boundary-crate helpers, header gen | Small communities; partial language coverage | MIT | 0.16.6 / 0.1.13 | Optional helper for the boundary crate (safer_ffi); not a generator |
| Java FFM for desktop JVM | No JNI glue; jextract | Java 22+ only; separate path from Android | GPL+CE (JDK) | Final since 22 | Defer |
| purego for Go | No cgo, easy cross-compile | Runtime dlopen; narrower ABI support | Apache-2.0 | 0.11.3 | Spike, optional |
| WASI 0.3 components | Native async, typed WIT interfaces | No native browser support; tooling experimental | Apache-2.0 w/ LLVM exc. | WASI 0.3.2 | Revisit 2027 for server-side WASM |

## 5. What we build ourselves, and why

1. **IDL extractor** (`teigeodata-idl`). Parses the public API crate (via `syn`, or a proc-macro that emits metadata) into a versioned JSON IDL: types, functions, errors, doc comments, plus structured tags (`#[geo(unit = "m", range = "0..=50000", example = "...")]` or doc-comment conventions). This is the single source for bindings, docs, MCP `inputSchema`/`outputSchema` and parity test vectors.
   *Proof:* the IDL round-trips (IDL → generated C header → cbindgen header diff is empty). Adding one function means touching only the core crate plus the hand-written idiomatic layer, if any.
2. **Boundary crate** (`teigeodata-ffi`). The designed C ABI: opaque handles, `tg_status` codes plus a thread-local last-error, `tg_str` (ptr, len) UTF-8, the columnar blob (`tg_blob { ptr, len, schema_id, release }`), callbacks (fn ptr, `void* ctx`, documented thread), and job handles for async. ABI version function plus semver rules. Panics are caught at the boundary.
   *Proof:* ABI compatibility check in CI (symbol and struct-layout diff against the last release).
3. **Generators** (Rust, template-driven, for example `minijinja`/`askama`). Outputs: C header, C++ wrapper, Swift, C# (`LibraryImport`), Go (cgo), Dart (`@Native`), Kotlin/Java JNI plus the Rust JNI shim, PyO3 shim, napi-rs shim, wasm-bindgen shim plus TS types, and MCP tool schemas.
   *Proof (spike S1):* generated plus hand-written lines per language, call overhead (noop and 1k-row search) within 2x of the best hand-written binding for that runtime, and binary size per feature variant.
4. **Columnar result blob and per-language decoders.** Format choice between Arrow C Data Interface / Arrow IPC, FlatBuffers and a minimal own layout (spike S2).
   *Proof:* decode 10k places with zero copies of numeric columns in JS, Python, Dart, Swift, C#, Kotlin and Go; measure ns/row and allocations.
5. **Parity harness.** Golden vectors (IDL-generated inputs plus expected outputs, JSON) run by a tiny runner in each language in CI. A byte-identical canonical output is required.
6. **Dart build hook** that downloads prebuilt binaries by target triple and verifies their checksum (and attestation, where possible), with a `native_toolchain_rust` fallback.

## 6. Impact on the plan

- **ADR (new): "Binding strategy".** Own C ABI plus IDL plus generators. Node, WASM, Python and JNI shims are Rust code generated from the same IDL. Diplomat and UniFFI are rejected, with reasons.
- **ADR (new): "Columnar result format"** (after spike S2).
- **ADR (new): "JVM and Android share JNI"**, with FFM deferred.
- **ADR (new): "MCP baseline 2026-07-28 on rmcp 3.x"**, covering transports, tool set and output limits.
- **Architecture:** boundary-crate page (ABI rules: memory ownership, threading, callbacks, async jobs, errors, versioning); IDL schema page; per-target packaging page.
- **Roadmap:** spikes S1-S8 come before any binding work. Swift distribution is SwiftPM-only (no CocoaPods). Android 16 KB check must be in CI before the first AAR (Play deadline 2027-02-01).
- **Gates:** parity gate (golden vectors in every language); ABI-compat gate; header-diff gate (generator vs cbindgen); 16 KB ELF gate; WASM size budget gate per variant; MCP conformance gate (2026-07-28 and 2025-11-25); Scorecard and provenance on every release.
- **Release pipeline:** release-plz (crates), cargo-dist (MCP server and CLI binaries), per-ecosystem publish jobs with OIDC trusted publishing where available, and `attest-build-provenance` on all artifacts.
- **Org admin:** verify the `com.teispace` namespace (DNS TXT on teispace.com); reserve npm scope `@teispace`, PyPI `teigeodata`, pub.dev `teigeodata` under a verified publisher, NuGet prefix reservation `Teispace.*`, and JSR scope.

## 7. Risks and open items

- **Generator maintenance** is ours forever. Mitigation: a small ABI surface, templates over ad-hoc code, snapshot tests (`insta`) of generated output, and a hand-written layer kept thin.
- **Threading and callbacks** differ per runtime: Dart isolates, Node event loop, Python GIL vs free-threaded, JNI attach/detach, Swift 6 Sendable. These are the most likely source of crashes and parity bugs, so they need a written threading contract plus stress tests in spike S1.
- **Free-threaded Python:** declaring `gil_used = false` requires the core to be `Sync`-safe through the shim. abi3t is new (3.15), so wheel tooling edge cases are likely.
- **Dart hooks:** stable, but data assets and some hook features are still evolving, and `native_toolchain_rust` is single-maintainer. Pin versions and keep our own download hook.
- **wasm-bindgen** governance moved orgs in 2025-26. It is active, but watch the bus factor. The fallback is generating raw `extern "C"` WASM exports plus our own JS glue (Diplomat-style) once the spec wasm C ABI is stable on stable Rust (**unverified status**).
- **rmcp churn:** a weekly cadence and a major version (3.0) in 2026-07. MCP deprecations (Sampling, Roots, Logging) carry a 12-month window. Pin a minor and track releases.
- **MCP client limits** on tool output are client-specific and **unverified**. Test with the main hosts.
- **Feature-variant explosion:** variants × platforms × ecosystems can blow up the matrix (macOS concurrency is 5). Limit published variants (for example `core`, `full`) and document build-from-source for custom variants.
- **Android 16 KB:** whether rustc's default page size is enough is **unverified**. Explicit link flags plus the CI check settle it.
- **Maven Central:** needs control of the `teispace.com` DNS and GPG key handling (no OIDC).
- **CocoaPods:** goes read-only on 2026-12-02. Users still on CocoaPods cannot get us. Accept this.
- **Cloudflare Workers size limits** are **unverified**. Check them before fixing WASM size budgets.
- **Go purego** ABI coverage (structs by value, callbacks) is **unverified**.
- **NuGet trusted publishing** is **unverified**.

**Spikes to run (with what to measure):**

| ID | Spike | Measure |
|---|---|---|
| S1 | IDL + boundary crate + generators for 3 functions (`search` → blob, `on_progress` callback, `download` async job) into C, Swift, Dart, Kotlin-JNI, Python (PyO3), Node (napi), WASM, C#, Go | Lines generated vs hand-written per language; noop call ns; 1k-row search end-to-end ns; callback latency from a Rust thread; time to add a 4th function |
| S2 | Columnar blob format: Arrow C Data / IPC vs FlatBuffers vs own | Decode ns/row and allocations for 10k rows in 7 languages; blob size; schema evolution story |
| S3 | WASM size and speed per feature variant | Raw, gz and br size after `wasm-opt -Oz`; init time; call overhead; Workers limit fit; view-vs-copy across `memory.grow` |
| S4 | Node: napi-rs prebuilds plus WASI fallback, run on Node 22/24/26, Bun and Deno | Install success matrix; load time; parity |
| S5 | Dart: download hook vs `native_toolchain_rust` on Flutter 3.47 (Android, iOS, macOS, Windows, Linux) plus Flutter web via the WASM build; link-hook tree-shaking | Build time, app size delta, symbols dropped by `record_use` |
| S6 | Android AAR: cargo-ndk + NDK r30, 16 KB check, JNI overhead vs UniFFI-JNA baseline | ELF LOAD/RELRO alignment; AAR size per ABI; call ns |
| S7 | Python: abi3 (3.10-3.15) and abi3t (3.15t) wheels via maturin/cibuildwheel; free-threaded stress test | Wheel count/size; throughput scaling with threads on 3.15t; parity |
| S8 | MCP server on rmcp 3.9: stdio plus stateless HTTP, tasks for download, MRTR elicitation, conformance suite with 2026-07-28 and 2025-11-25 clients | Conformance pass rate; response tokens per tool at default limits; p50/p99 latency |
| S9 | CI matrix dry run on GitHub (public repo) | Wall-clock per release; macOS queueing; cache hit rate; cargo-zigbuild glibc 2.17 output works on old distros |

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Rust stable | 1.99.1 (2026-10-13) | 2026-11-06 | static.rust-lang.org channel |
| diplomat / diplomat-tool | 0.17.0 (2026-10-20) | 2026-11-06 | crates.io API |
| uniffi | 0.32.2 (2026-09-22) | 2026-11-06 | crates.io API, CHANGELOG |
| uniffi-bindgen-go | v0.7.1+v0.31.0 | 2026-11-06 | GitHub releases (NordSecurity) |
| uniffi-bindgen-cs | v0.11.1+v0.31.0 | 2026-11-06 | GitHub releases (NordSecurity) |
| uniffi-bindgen-java | 0.5.2 | 2026-11-06 | crates.io API |
| uniffi-bindgen-react-native | 0.31.0-6 | 2026-11-06 | GitHub releases |
| boltffi | 0.32.1 | 2026-11-06 | crates.io API |
| napi / napi-derive | 3.16.1 / 3.7.0 | 2026-11-06 | crates.io API |
| @napi-rs/cli | 3.10.12 (2026-11-03) | 2026-11-06 | npm registry |
| node-addon-api | 8.9.2 | 2026-11-06 | npm registry |
| Node.js | 26.13.0 LTS, 24.22.2 LTS (maint.), 22.23.4 (maint.), 27 alpha | 2026-11-06 | nodejs.org dist index, Release schedule |
| Node-API | 10 (highest documented) | 2026-11-06 | nodejs.org/api/n-api.html |
| wasm-bindgen / CLI | 0.2.130 (2026-10-30) | 2026-11-06 | crates.io API, GitHub `wasm-bindgen` org |
| wasm-pack | 0.15.1 (2026-10-30) | 2026-11-06 | crates.io API, GitHub |
| binaryen (wasm-opt) | version_133 (2026-09-21) | 2026-11-06 | GitHub releases |
| WASI | 0.3.2 (0.3.0 ratified 2026-06-11) | 2026-11-06 | GitHub WebAssembly/WASI releases |
| wasmtime | v50.0.1 | 2026-11-06 | GitHub releases |
| wit-bindgen | 0.63.0 | 2026-11-06 | crates.io API |
| cargo-component | 0.21.1 (experimental) | 2026-11-06 | crates.io, README |
| @bytecodealliance/jco | 1.41.0 | 2026-11-06 | npm registry |
| Flutter stable | 3.47.9 (2026-10-29) | 2026-11-06 | Flutter releases JSON |
| Dart stable | 3.13.7 (2026-10-22) | 2026-11-06 | dart-archive VERSION; SDK CHANGELOG |
| hooks / code_assets | 2.2.0 / 2.2.0 | 2026-11-06 | pub.dev API |
| ffigen / ffi | 23.0.0 / 2.2.0 | 2026-11-06 | pub.dev API |
| native_toolchain_rust | 1.0.7 (2026-09-10) | 2026-11-06 | pub.dev |
| flutter_rust_bridge | 2.13.0 | 2026-11-06 | crates.io, pub.dev |
| jni (Dart) / jnigen | 1.1.2 / 1.0.1 | 2026-11-06 | pub.dev API |
| Python | 3.15.2, 3.14.10 | 2026-11-06 | python.org API |
| pyo3 | 0.30.1 | 2026-11-06 | crates.io API, CHANGELOG |
| maturin | 1.16.0 | 2026-11-06 | crates.io, PyPI |
| cibuildwheel | 4.3.5 | 2026-11-06 | PyPI |
| JDK | 27 feature, 25 LTS | 2026-11-06 | Adoptium API |
| jni (Rust) | 0.22.4 | 2026-11-06 | crates.io API |
| cargo-ndk | 4.1.2 | 2026-11-06 | crates.io API |
| Android NDK | r30 (30.0.16248370) | 2026-11-06 | GitHub android/ndk, SDK repository XML |
| Swift | 6.4.2 (2026-10-30) | 2026-11-06 | GitHub swiftlang/swift releases |
| swift-bridge | 0.1.59 | 2026-11-06 | crates.io API |
| .NET | 10.0.14 (LTS), 11.0 RC 3 | 2026-11-06 | NuGet (Microsoft.NETCore.App.Ref) |
| csbindgen | 1.9.8 | 2026-11-06 | crates.io API |
| Go | 1.27.3, 1.26.10 | 2026-11-06 | go.dev/dl JSON |
| purego | v0.11.3 (2026-11-01) | 2026-11-06 | proxy.golang.org |
| cbindgen | 0.29.4 | 2026-11-06 | crates.io API |
| interoptopus | 0.16.6 | 2026-11-06 | crates.io API |
| safer-ffi | 0.1.13 | 2026-11-06 | crates.io API |
| cargo-zigbuild | 0.24.1 (2026-11-04) | 2026-11-06 | crates.io, GitHub |
| cross | 0.2.5 (2023-02-04) | 2026-11-06 | crates.io, GitHub |
| cargo-dist | 0.34.0 (2026-11-02) | 2026-11-06 | crates.io, GitHub |
| release-plz | 0.4.3 (2026-11-02) | 2026-11-06 | crates.io, GitHub |
| actions/attest-build-provenance | v4.2.2 | 2026-11-06 | GitHub releases |
| ossf/scorecard-action | v2.5.0 (scorecard v5.6.0) | 2026-11-06 | GitHub releases |
| sigstore/cosign | v3.1.4 | 2026-11-06 | GitHub releases |
| actions/upload-artifact / actions/cache | v7.0.2 / v6.1.0 | 2026-11-06 | GitHub releases |
| MCP protocol | 2026-07-28 (current) | 2026-11-06 | modelcontextprotocol.io versioning |
| rmcp | 3.9.0 (2026-11-05), MSRV 1.88 | 2026-11-06 | crates.io API, GitHub releases |
| @modelcontextprotocol/sdk (TS) | 1.32.2 | 2026-11-06 | npm registry |
| mcp (Python SDK) | 2.3.0 | 2026-11-06 | PyPI |

## 9. Sources

- [crates.io API](https://crates.io/api/v1/crates/), crates.io, accessed 2026-11-06 (versions, licences, MSRV for all crates above).
- [Diplomat README](https://github.com/rust-diplomat/diplomat), rust-diplomat, accessed 2026-11-06.
- [UniFFI CHANGELOG](https://github.com/mozilla/uniffi-rs/blob/main/CHANGELOG.md), Mozilla, accessed 2026-11-06.
- [BoltFFI README](https://github.com/boltffi/boltffi), BoltFFI, accessed 2026-11-06.
- [NAPI-RS v3 announcement](https://napi.rs/blog/announce-v3), napi-rs, published 2025-07-07.
- [Node-API documentation](https://nodejs.org/api/n-api.html), Node.js, accessed 2026-11-06.
- [Node.js release schedule](https://github.com/nodejs/Release/blob/main/schedule.json), Node.js, accessed 2026-11-06.
- [wasm-bindgen repository](https://github.com/wasm-bindgen/wasm-bindgen) and [wasm-pack](https://github.com/wasm-bindgen/wasm-pack), accessed 2026-11-06.
- [WASI 0.3.0 release notes](https://github.com/WebAssembly/WASI/releases/tag/v0.3.0), WebAssembly CG, published 2026-06-11.
- [Dart SDK CHANGELOG](https://github.com/dart-lang/sdk/blob/main/CHANGELOG.md), Dart, accessed 2026-11-06 (3.10.0 hooks stable; 3.13.0 released 2026-08-12).
- [Hooks](https://dart.dev/tools/hooks), dart.dev, accessed 2026-11-06.
- [Bind to native code using FFI](https://docs.flutter.dev/platform-integration/bind-native-code), Flutter docs (3.47), accessed 2026-11-06.
- [native_toolchain_rust](https://pub.dev/packages/native_toolchain_rust), pub.dev, accessed 2026-11-06.
- [PyO3 CHANGELOG](https://github.com/PyO3/pyo3/blob/main/CHANGELOG.md) and [maturin Changelog](https://github.com/PyO3/maturin/blob/main/Changelog.md), PyO3, accessed 2026-11-06.
- [PEP 803 – abi3t](https://peps.python.org/803) and [acceptance thread](https://discuss.python.org/t/pep-803-round-two-abi3t-stable-abi-for-free-threaded-builds/106181?page=2), Python, accessed 2026-11-06.
- [Maven Central requirements](https://central.sonatype.org/publish/requirements/) and [namespace registration](https://central.sonatype.org/register/namespace/), Sonatype, accessed 2026-11-06.
- [Support 16 KB page sizes](https://developer.android.com/guide/practices/page-sizes), Android Developers, accessed 2026-11-06.
- [CocoaPods Specs Repo (trunk read-only)](https://blog.cocoapods.org/CocoaPods-Specs-Repo/), CocoaPods blog, accessed 2026-11-06.
- [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), GitHub Docs, accessed 2026-11-06.
- [Actions limits](https://docs.github.com/en/actions/reference/limits), GitHub Docs, accessed 2026-11-06.
- [About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases), GitHub Docs, accessed 2026-11-06.
- [actions/attest-build-provenance](https://github.com/actions/attest-build-provenance), GitHub, accessed 2026-11-06.
- [MCP versioning](https://modelcontextprotocol.io/specification/versioning), [2026-07-28 key changes](https://modelcontextprotocol.io/specification/2026-07-28/changelog), [Tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools), [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http), Model Context Protocol, accessed 2026-11-06.
- [rmcp releases](https://github.com/modelcontextprotocol/rust-sdk/releases) and [README](https://github.com/modelcontextprotocol/rust-sdk), MCP Rust SDK, accessed 2026-11-06 (3.0.0 published 2026-07-28; 3.9.0 published 2026-11-05).
- [LLVM clang Android max-page-size change](https://github.com/llvm/llvm-project/pull/87413), LLVM, accessed 2026-11-06.
