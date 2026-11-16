---
title: "Dart and Flutter: @Native bindings and a build hook of our own"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# Dart and Flutter: @Native bindings and a build hook of our own

## 1. Question

Phase 5.5 ships the Dart package so that a Dart or Flutter app gets the native library with no
manual step, on Android, iOS, macOS, Windows and Linux (the web waits on the WASM build, 5.7).
The strategy is in the [bindings note](bindings-and-mcp.md), §3.4: build hooks, a verified
prebuilt download, a build from source as the fallback. Spike S5 asks:

- `@Native` or `DynamicLibrary` lookups, at what call cost;
- our own hook or `native_toolchain_rust`;
- how the hook finds the library in the repository, for a user who builds from source, and
  from a release;
- whether Flutter really bundles it on each platform.

## 2. Summary and recommendation

- **`@Native` externals under one `@DefaultAsset`.** The generated `bindings.g.dart` declares
  each C function as a top-level `@Native` external of the asset
  `package:teigeodata/src/bindings.g.dart`. `TeiGeodata()` now only holds the scratch memory
  its calls reuse. There is no path to open, and a helper isolate (`...Async`) resolves the
  same functions instead of reopening the library by path. Handle finalizers take the free
  function's address with `Native.addressOf`.
- **Call cost is unchanged at the API level.** Measured 2026-11-14 on this Mac (M-series,
  AOT, `dart build cli`; machine load 8 to 16):

  | Call | `lookupFunction` (before) | `@Native` (after) |
  |---|---|---|
  | `noop` through the API | 6.1 ns | 5.9 to 6.2 ns |
  | `fold("Kathmandu")` | 79 to 81 ns | 78 to 84 ns |
  | `sample_rows(1000)` plus a column sum | 23.0 to 23.5 µs | 24.1 to 25.2 µs |
  | callback, per call | 28 to 32 ns | 29 ns |
  | raw leaf call, same process | 2.2 to 2.3 ns | 2.7 to 2.9 ns |

  The raw `@Native` call is about 0.5 ns dearer, which looks like the lazily resolved address,
  and is lost in the API's own work. The row and fold differences are within the noise of a
  loaded machine.
- **Our own hook, not `native_toolchain_rust`.** `hook/build.dart` (with `lib/src/hook/`, about 200
  lines) picks the library, first match wins:
  1. `hooks.user_defines.teigeodata.library`: a file already built (any variant, lite included);
  2. `hooks.user_defines.teigeodata.source`, or the package sitting in the repository: `cargo
     build --release --locked -p teigeodata-c`, in the repository's own `target/` (it shares
     the cache with every other build), with `--target` only when cross-building;
  3. otherwise, or with `prebuilt: true`: the release's library for the target, downloaded
     once into the hook's shared output folder and used only if its SHA-256 matches the one
     pinned in `lib/src/hook/prebuilt_hashes.dart`. `prebuilt_base` points at a mirror.

  pub.dev accepts only `hook/build.dart` and `hook/link.dart` under `hook/` ("Hook files are
  experimental and `hook/src/prebuilt.dart` is not allowed yet", its server on 2026-11-16; the
  local `dart pub publish --dry-run` does not check it), so the helpers live in
  `lib/src/hook/` and the hook imports them as `package:teigeodata/src/hook/…` (D-361).
  The hook's dependencies are the cargo `.d` file's sources and `Cargo.lock`, so an unchanged
  build is skipped: a second `dart test` took 2 s.
- **Flutter bundles it on every platform tried.** `cargo xtask flutter-check` makes the
  example's platform folders with `flutter create .` and runs its integration test (calls, a
  callback, a helper isolate, a widget). Measured 2026-11-14 with Flutter 3.50.0: macOS 64 s
  from clean, wrapped as `teigeodata.framework`; the iOS simulator (iPhone 18 Pro Max) 5 min
  20 s, almost all of it Xcode. CI adds Linux, Windows and an Android emulator (`flutter.yml`).
- **Proven by planted faults.** A wrong asset name fails every call with "No asset with id";
  a wrong expected value fails `flutter-check`; one changed byte in a mirrored prebuilt is
  refused by its hash, and no partial file is left.

## 3. Findings

- **Dart rewrites a dylib's install name** to an absolute path when it bundles it, and
  `install_name_tool` failed: the Mach-O header had no room. `teigeodata-c` now links Apple
  builds with `-headerpad_max_install_names`, which any packager that renames the library
  needs.
- **`native_toolchain_rust` 1.0.7 does not fit.** It links Android with the NDK's
  `<triple>35-clang`, so the library may need libc symbols newer than an app's minSdk (ours is
  24). It also wants the crate inside the Dart package and builds into a per-configuration
  target folder. Our hook links with `<triple><minSdk>-clang` from the NDK that Flutter
  passes in, and builds the repository's crate in place.
- **Hooks run in a reduced environment.** PATH and HOME pass, so `cargo` and rustup's
  toolchain from `rust-toolchain.toml` work. On macOS the hook drops Xcode's
  `/Contents/Developer/` entries from PATH: a build script that compiles for the host would
  otherwise get the target's SDK.
- **A missing Rust target** fails with the `rustup target add` line to run. The toolchain file
  does not list targets, so a desktop-only developer downloads none.
- **Flutter 3.50 deprecates `package:flutter/material.dart`** in favour of `material_ui`. The
  example uses `widgets.dart` only. It enables Swift Package Manager in its own pubspec
  (`flutter: config:`), so no CocoaPods is needed, whatever the machine's global setting.
- **`dart compile exe` is replaced by `dart build cli`** for a program with hooks; it bundles
  the library at `bundle/lib/`.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `@Native` plus our hook | One generated file; isolates share it; minSdk respected; prebuilt or source; small | Our hook to maintain (about 200 lines) | ours | built 2026-11-14 | **Chosen** |
| `@Native` plus `native_toolchain_rust` | Little code | Android linked at API 35; crate must sit in the package; single maintainer | MIT | 1.0.7 | Rejected (§3) |
| `DynamicLibrary` lookups (S4 layer) | 0.5 ns cheaper raw call | A path to find on every platform; helper isolates reopen it; no hook or tree-shaking | n/a | S4 | Replaced |
| `flutter_rust_bridge` | Productive for apps | Its own wire format; Dart only | MIT | 2.13.0 | Rejected (bindings note) |

## 5. What we build ourselves, and why

`bindings/dart/hook/` (the hook, the target table, the prebuilt download and its hashes),
`bindings/dart/example/` (sources only; the platform folders are generated) and
`xtask/src/flutter_check.rs`. Nothing on pub.dev builds a Rust workspace crate at an app's
minSdk.

## 6. Impact on the plan

- [Phase 5](../../07-planning/phases/phase-5.md) 5.5: 5.5a (`@Native`, the hook from source)
  and 5.5c (Flutter on each platform) here; 5.5b's download is built and proven against a
  local mirror, and its hashes are written by the release pipeline (5.10); 5.5d, link-hook
  tree-shaking, needs static linking (below); the web waits on 5.7.
- D-289.

## 7. Risks and open items

- **Tree-shaking (5.5d).** A link hook can drop unused native code only when the library is
  linked statically (`StaticLinking`), which Dart and Flutter do not yet support on every
  platform; `record_use` 1.1.1 already reports which `@Native` functions an app calls. To
  measure when static linking lands; until then the lite library is the small variant.
- **Prebuilt assets.** Their names (`teigeodata-<version>-<triple>-<file>`) and hashes are
  fixed here; the release workflow must upload exactly those files and write the hashes before
  `dart pub publish`.
- **A published package's source fallback** needs a checkout (`source:`): the Rust workspace
  is not shipped inside the pub package.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Dart | 3.14.0 | 2026-11-14 | `dart --version` |
| Flutter | 3.50.0 | 2026-11-14 | `flutter --version` |
| hooks / code_assets | 2.2.0 / 2.2.0 | 2026-11-14 | pub.dev API |
| native_toolchain_rust | 1.0.7 | 2026-11-14 | pub.dev API, source read |
| record_use | 1.1.1 | 2026-11-14 | pub.dev API |
| crypto | 3.0.8 | 2026-11-14 | pub.dev API |
| subosito/flutter-action | v2.23.0 | 2026-11-14 | GitHub API |

## 9. Sources

- [Hooks](https://dart.dev/tools/hooks), dart.dev, accessed 2026-11-06 ([bindings note](bindings-and-mcp.md)).
- `hooks` 2.2.0 and `code_assets` 2.2.0 READMEs and sources (`CodeConfig`, `CodeAsset`,
  link modes, native library validators), pub.dev, read 2026-11-14.
- `native_toolchain_rust` 1.0.7 sources (`build_runner.dart`, `build_environment.dart`,
  `config_mapping.dart`), pub.dev, read 2026-11-14.
- [Bind to native code using FFI](https://docs.flutter.dev/platform-integration/bind-native-code), Flutter docs, accessed 2026-11-06.
