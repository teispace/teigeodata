---
title: ".NET and Go over the C ABI: how each loads the library, and from where"
status: research
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# .NET and Go over the C ABI: how each loads the library, and from where

## 1. Question

Phase 5.8 ships .NET ("source-generated P/Invoke, NativeAOT-safe, as a NuGet package") and Go
("over cgo, with purego as an option if S1 shows it works"), plus C/C++ examples
([phase 5](../../07-planning/phases/phase-5.md)). The [bindings note](bindings-and-mcp.md)
§3.9 to §3.11 chose the mechanisms. Open:

- whether purego covers our ABI's shapes, and so which Go path is the default;
- where a Go program gets the native library, since a Go module carries source;
- which .NET target frameworks and runtime identifiers the package carries;
- what each is tested on.

## 2. Summary and recommendation

- **Go: purego by default, cgo behind a build tag.** purego 0.11.3 has every desktop
  platform we build at Tier 1 and passes structs by value there; our one callback type takes
  scalars, which Windows also supports. With purego a Go program needs no C toolchain and
  cross-compiles with `CGO_ENABLED=0`; the cost is a call through purego's trampoline
  (**to be measured, S1**: noop and a 1k-row search against cgo, held to the 2x rule of the
  bindings note).
- **The Go library comes from the C package** (D-286), the asset a release already publishes
  per platform: the binding finds `libteigeodata` from an explicit path, then
  `TEIGEODATA_LIB`, then a per-user cache filled by a download pinned by SHA-256 in the Go
  source (as Dart's hook does, D-289). The cgo tag links the same package through its
  pkg-config file. No binaries in the module, no second repository.
- **.NET: one NuGet package, `net10.0`** (the LTS; .NET 11, STS, runs it), `LibraryImport`
  generated from the IDL, `SafeHandle` per handle, the blob as `ReadOnlySpan<byte>`, with the
  library under `runtimes/<rid>/native/` for `osx-arm64`, `osx-x64`, `linux-x64`,
  `linux-arm64`, `linux-musl-x64`, `linux-musl-arm64`, `win-x64` and `win-arm64`. Tested from
  the packed `.nupkg` on each, and published NativeAOT on one.
- **musl in the C package:** .NET's `linux-musl-*` and Alpine Go programs need it; the Node.js
  packages already build musl in Alpine (D-294). The C package gains both musl targets.
- **C/C++:** the C package's `parity.c` is the C example; a generated header-only C++17
  wrapper (RAII handles, `std::span` over blobs) with its own example and test.

## 3. Findings

- **purego platforms (README, v0.11.3, read 2026-11-14).** Tier 1: Linux, macOS and Windows
  on amd64 and arm64 (plus Android and iOS, which need cgo). Structs by value as arguments
  and returns work on all Tier 1 desktop platforms; on Windows not inside `NewCallback`
  callbacks. Our ABI passes one struct by value (`tg_bytes_free(tg_bytes)`), option structs
  by pointer, and one callback type, `bool (*)(void *, uint32_t)`.
- **purego loads with `dlopen`**, so a library must exist on disk at run time; with
  `CGO_ENABLED=0` on Linux it uses its `fakecgo` runtime. Whether that works on musl is
  **unverified** (the README does not name musl).
- **Go versions:** 1.27.4 and 1.26.11 are supported (go.dev/dl, 2026-11-14). Go keeps the two
  newest releases; the module's `go` line is 1.26.
- **.NET versions (releases-index.json, 2026-11-14):** 11.0.0 (STS, released this month) and
  10.0.15 (LTS) are active, both to 2028-11-14; 8 and 9 reached their end on 2026-11-10.
  `LibraryImport` needs .NET 7 or later, so `netstandard2.0` (Unity, .NET Framework) would
  need a second, `DllImport` form (**open question**, not in v1's first cut).
- **NuGet trusted publishing** works from GitHub Actions through OIDC (bindings note §8); the
  package must exist before a policy can name it, so the first upload uses a key.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Go via purego, library from the C package | No C toolchain; cross-compiles; same assets as C and Dart | Runtime `dlopen`; per-call trampoline cost (S1); musl unverified | Apache-2.0 | 0.11.3 | **Default** |
| Go via cgo, static library per platform in the module | Fastest calls; one binary | C toolchain for every user; binaries in a module (tens of MB per platform) or a second repository | n/a | Go 1.26+ | Rejected as default |
| Go via cgo and the C package's pkg-config | Fastest calls; no binaries in the module | C toolchain; the user installs the C package | n/a | Go 1.26+ | **Build tag `teigeodata_cgo`** |
| .NET `LibraryImport`, one NuGet package with every RID | Trimming and NativeAOT safe; NuGet picks the RID | `net7.0`+ only | MIT (runtime) | .NET 10 LTS | **Chosen** |
| .NET `DllImport` | Runs on `netstandard2.0` (Unity, Framework) | Runtime marshalling stubs; not NativeAOT-clean | MIT | all | Open question |
| A package per RID plus a meta package | Smaller downloads | NuGet has no `optionalDependencies`; RID packages need a runtime.json graph | n/a | n/a | Rejected (about 3 MB a RID) |

## 5. What we build ourselves, and why

- **Go and C# templates** from the IDL, like every other layer, and their parity tests over
  `bindings/parity.json`.
- **A Go loader** (path, environment, pinned download into the user cache), the same contract
  as the Dart hook and the JVM `NativeLoader`.
- **`cargo xtask package-dotnet`** assembling the `.nupkg` from the C package's libraries.

## 6. Impact on the plan

- 5.8 splits: 5.8a the C package's musl targets; 5.8b Go (S1 first); 5.8c .NET; 5.8d the C++
  wrapper and examples.
- A decision per binding when it is proven.

## 7. Risks and open items

- **S1 is not measured yet**: purego's call cost against cgo, and musl under purego.
- **`netstandard2.0`** for Unity and .NET Framework: a second form, or not in v1.
- **Neither Go nor the .NET SDK is installed on this machine**; until they are, these
  bindings are built and tested in CI only.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Go | 1.27.4, 1.26.11 | 2026-11-14 | go.dev/dl JSON |
| purego | v0.11.3 (2026-11-01) | 2026-11-14 | proxy.golang.org |
| .NET | 11.0.0 (STS), 10.0.15 (LTS) | 2026-11-14 | dotnet release-metadata releases-index.json |

## 9. Sources

- [purego README, v0.11.3](https://github.com/ebitengine/purego/blob/v0.11.3/README.md),
  "Supported Platforms", read 2026-11-14.
- [Go downloads](https://go.dev/dl/?mode=json), accessed 2026-11-14.
- [.NET release index](https://dotnetcli.blob.core.windows.net/dotnet/release-metadata/releases-index.json),
  accessed 2026-11-14.
