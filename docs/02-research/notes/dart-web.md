---
title: "Dart on the web: the Dart package over the WebAssembly build"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-01-15
---

# Dart on the web

## 1. Question

How the Dart package (phase 5.5) runs in a browser, compiled by dart2js or dart2wasm (Flutter
web uses either), on the WebAssembly build of phase 5.7, with the API the native package
has, so one app's code compiles for every platform. Feeds the last item of 5.5 and M5's
"the web, with the WASM build".

## 2. Summary and recommendation

A third generated Dart file, `bindings_web.g.dart`, implements the native layer's classes
(`TeiGeodata`, `Engine`, the exceptions, enums and option records) over the module of
`teigeodata-wasm` through `dart:js_interop`. The package exports it in place of
the native layer with a conditional export (`if (dart.library.js_interop)`). The types both
layers share move to `types.g.dart`, so they have one definition. The only addition to the
API is `TeiGeodata.init({Uri? module})`: on the web it loads the module (once), natively it
returns at once, so portable code awaits it everywhere. The module loads from the app's own
origin by default (`teigeodata_wasm/index.js` beside the page), never from a CDN unasked
(offline-first, no third party). As in the module, the web leaves out what needs a file
system or threads (`openFile*`, the store and its jobs); packs open from bytes.

Measured in Chrome with dart2js and dart2wasm (Dart 3.14.0): the parity checks the native
suite runs pass on both, through the same check code (`test/src/checks.dart`).

## 3. Findings

**Interop (Dart 3.14.0, checked 2026-11-15 in Chrome 159 by experiment):**

- `importModule(JSAny)` (`dart:js_interop`) is JavaScript's `import()`, returning
  `JSPromise<JSObject>`. **dart2js bug:** when the call is inlined into an async function,
  dart2js's async rewrite hoists `import` itself into a temporary (`$async$temp2 = import;`),
  a syntax error that stops the whole script loading ("Cannot use import statement outside a
  module"). A synchronous helper marked `@pragma('dart2js:noInline')` avoids it; without the
  pragma the inliner brings the bug back.
- External members of an extension type take and return Dart `String`, `int`, `double` and
  `bool` directly; bytes cross as `JSUint8Array` (`.toJS` and `.toDart`). A JavaScript class
  is constructed with `callAsConstructor` (`dart:js_interop_unsafe`). An options object is an
  extension type with an `external factory` of named parameters (an object literal).
- A JavaScript `Error` thrown by the module arrives in a Dart `catch` as itself under dart2js
  and as a JavaScript value under dart2wasm. `JSError` is in `package:web`, not
  `dart:js_interop`, so the layer declares its own `@JS('Error')` extension type and tests
  `isA<…>()` (an `instanceof`). A Dart object is told apart differently by each compiler:
  under dart2js `as JSAny?` succeeds and `isA<Error>` is false, and under dart2wasm the cast
  itself throws a `TypeError`. The layer rethrows Dart errors and exceptions unchanged, then
  casts and tests.
- A callback (`progress`) is a Dart closure through `.toJS`. Its exception is caught in Dart,
  the callback answers false and the exception is rethrown after the call, as the native layer
  does; it never crosses into JavaScript.
- The module's classes register a `FinalizationRegistry` (wasm-bindgen 0.2.130), so an
  `Engine` never closed is freed when collected, as `NativeFinalizer` does natively.
- dart2js has no `Int64List` and no `ByteData.getInt64`: a reader of a blob's 64-bit
  integer columns combines the two 32-bit halves (exact to 2^53, as JavaScript numbers are).
  The test's blob reader does this on every platform.

**Blob results, copied or not (measured 2026-11-15, Apple M-series, Chrome 159, 20 rounds of
`sampleRows(100000)` and a sum over its `lat` column):**

| Compiler | `toDart` view | `Uint8List.fromList` copy |
|---|---|---|
| dart2wasm | 182–195 ms | 503–517 ms |
| dart2js | 147–154 ms | 157–168 ms |

Under dart2wasm the view backed by JavaScript memory is 2.7 times faster than a copy into
Wasm memory, and under dart2js it is a view with no copy at all. The layer returns `toDart`.

**Serving in tests:** `dart test -p chrome` serves the package's own tree only. The suite
starts a server on the VM with `spawnHybridUri` (`test/web_server.dart`); it serves the built
WebAssembly package with CORS and sends the page `parity.json` and the fixture packs over the
channel. A number crosses that channel as a `double` under dart2wasm. `TEIGEODATA_PARITY` names
another copy of the expectations, as for the native suite.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Generated `dart:js_interop` layer over the published WASM module, conditional export | One Rust build for JavaScript and Dart; the same IDL; dart2js and dart2wasm; no extra toolchain | The app serves the module's files | Apache-2.0 | `dart:js_interop` stable since Dart 3.3 | **Chosen** |
| Dart's own FFI on the web (`dart:ffi` under dart2wasm) | One layer | Not supported in the browser; dart2js never | n/a | Not available | Rejected |
| Hand-written web layer | No template | Diverges from the native API at the first change; fails P2 (generated bindings) | n/a | n/a | Rejected |
| Bundle the module inside the pub package (`lib/`) | Nothing for the app to copy | pub omits git-ignored build output; Flutter web serves package assets only when the app lists them; every app downloads every variant | n/a | n/a | Rejected for now; the README says how to serve the npm package |

## 5. What we build ourselves, and why

The template `xtask/templates/dart_web.j2` (and `dart_types.j2`, the shared types), so the web
layer stays in step with the IDL like every other binding. The web suite runs the shared
checks under both compilers in CI (`wasm.yml`, job `dart-web`). Planted faults proven caught
on 2026-11-15: a changed expectation (2 checks fail) and a broken error translation (5 fail).

## 6. Impact on the plan

- Phase 5.5's web item is done (D-307).
- `bindings.md` §8: Dart on the web.
- The package README: serving the module.
- The parity-mutants gate runs the native Dart suite; the web suite runs the same check code
  (`test/src/checks.dart`), so a mutant the native suite kills in those checks is killed on
  the web as well. The web suite is not a separate entry of the gate.

## 7. Risks and open items

- The dart2js `import()` bug: recheck on each Dart release and drop the pragma when fixed
  (revisit 2027-01-15).
- The app must serve `index.js`, `teigeodata_wasm.js` and `teigeodata_wasm_bg.wasm` from one
  folder (the wasm file as `application/wasm` for streaming compilation).
- Flutter web was not run in a Flutter app here: the package is plain Dart on the web, tested
  under the two compilers Flutter web uses.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Dart SDK | 3.14.0 | 2026-11-15 | `dart --version` |
| Chrome (test runner) | 159 | 2026-11-15 | the test runner's log |
| wasm-bindgen | 0.2.130 | 2026-11-15 | `bindings/wasm/Cargo.toml` |
| `stream_channel` | 2.1.4 | 2026-11-15 | pub.dev API |

## 9. Sources

- [dart:js_interop library](https://api.flutter.dev/flutter/dart-js_interop/), Dart API
  reference, accessed 2026-11-15.
- [JS types](https://dart.dev/interop/js-interop/js-types), dart.dev, accessed 2026-11-15.
- [invalid_runtime_check_with_js_interop_types](https://dart.dev/lints/invalid_runtime_check_with_js_interop_types),
  dart.dev lint docs (catching JavaScript errors with `isA`), accessed 2026-11-15.
- [js_exceptions_test.dart](https://dart.googlesource.com/sdk/+/stable/tests/web/wasm/js_exceptions_test.dart),
  Dart SDK, accessed 2026-11-15.
- [dart-lang/sdk#55457](https://github.com/dart-lang/sdk/issues/55457), dart2wasm JavaScript
  exceptions in async functions (fixed), accessed 2026-11-15.
- Experiment: `dart test -p chrome --compiler dart2js,dart2wasm`, 2026-11-15 (this note §3).
